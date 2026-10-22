use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use tracing::{debug, trace};

use crate::error::{NetherNetError, catch_dependency_panic};
use crate::signaling::{
    IceServer, Signal, SignalErrorCode, Signaling, parse_ice_candidate, signal_type,
};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};
use crate::transport::session::Role;
use crate::transport::{Connection, LocalIdentity, Pending, TransportConfig, sdp};

/// Configuration for `NetherNetDialer`.
#[derive(Clone)]
pub struct NetherNetDialerConfig {
    /// Configuration for streams created by this dialer.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// Explicit ICE servers; nonempty values override signaling credentials.
    pub ice_servers: Vec<IceServer>,
    /// Local interface/ICE policy; the default matches go-nethernet's pion
    /// defaults (all non-loopback UDP4/UDP6 addresses).
    pub transport: TransportConfig,
    /// Local signed SDP identity (identity.go:Identity).
    pub identity: Option<Arc<crate::identity::Identity>>,
    /// Optional server-token policy; the default verifies self-signed server tokens.
    pub verify_server_token: Option<Arc<dyn crate::identity::TokenVerifier>>,
    /// Explicit local/testing opt-out; upstream dial.go rejects identityless answers by default.
    pub allow_identityless_server: bool,
}

impl Default for NetherNetDialerConfig {
    fn default() -> Self {
        Self {
            stream_config: NetherNetStreamConfig::default(),
            // df-mc/go-nethernet dial.go:Dialer.Dial uses one 15-second deadline.
            connection_timeout: Duration::from_secs(15),
            // credentials.go:gatherOptions: no implicit public STUN service.
            ice_servers: Vec::new(),
            transport: TransportConfig::default(),
            identity: None,
            verify_server_token: None,
            allow_identityless_server: false,
        }
    }
}

/// A dialer for establishing NetherNet connections.
///
/// Similar to `RaknetStream::connect`, this spawns a background actor to handle
/// signaling dispatch and connection negotiation.
pub struct NetherNetDialer {
    request_tx: mpsc::Sender<DialerRequest>,
}

enum DialerRequest {
    Dial {
        network_id: String,
        response_tx: oneshot::Sender<Result<NetherNetStream, NetherNetError>>,
    },
}

struct DialerActor {
    signaling: Arc<dyn Signaling>,
    config: NetherNetDialerConfig,
    signal_rx: mpsc::Receiver<Signal>,
    request_rx: mpsc::Receiver<DialerRequest>,
    pending_dials: std::collections::HashMap<(String, u64), mpsc::Sender<Signal>>,
    /// DTLS certificate shared by this dialer's connections (generated on first dial).
    local: Option<LocalIdentity>,
}

impl NetherNetDialer {
    /// Creates a new dialer with the given signaling implementation.
    ///
    /// Returns the dialer and a channel sender for dispatching received signals.
    /// The caller is responsible for routing incoming network signals to this sender.
    ///
    /// # Background Task
    /// This spawns a background tokio task (the `DialerActor`) that handles:
    /// - Dispatching `dial()` requests
    /// - Processing incoming `CONNECTRESPONSE` signals
    /// - ICE candidate exchange
    pub fn new(
        signaling: Arc<dyn Signaling>,
        config: NetherNetDialerConfig,
    ) -> (Self, mpsc::Sender<Signal>) {
        let (signal_tx, signal_rx) = mpsc::channel(128);
        let (request_tx, request_rx) = mpsc::channel(64);

        let actor = DialerActor {
            signaling,
            config,
            signal_rx,
            request_rx,
            pending_dials: std::collections::HashMap::new(),
            local: None,
        };

        tokio::spawn(actor.run());

        (Self { request_tx }, signal_tx)
    }

    /// Creates a dialer using a signaling channel that implements SignalingChannel.
    ///
    /// This is the preferred constructor for ergonomic usage - it internalizes
    /// the signal routing, eliminating boilerplate in user code.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let discovery = DiscoveryListener::bind("0.0.0.0:7551", config).await?;
    /// let dialer = NetherNetDialer::connect_with_signaling(
    ///     discovery,
    ///     NetherNetDialerConfig::default()
    /// );
    /// let stream = dialer.dial(network_id).await?;
    /// ```
    pub fn connect_with_signaling<S>(signaling: S, config: NetherNetDialerConfig) -> Self
    where
        S: crate::signaling::SignalingChannel + 'static,
    {
        let (signal_tx, signal_rx) = mpsc::channel(128);
        let (request_tx, request_rx) = mpsc::channel(64);

        let signaling = std::sync::Arc::new(signaling);
        let signaling_clone = signaling.clone();

        let actor = DialerActor {
            signaling: signaling.clone(),
            config,
            signal_rx,
            request_rx,
            pending_dials: std::collections::HashMap::new(),
            local: None,
        };

        // Subscribe before starting the actor: HTTP offers can synchronously produce an answer.
        tokio::spawn(async move {
            if let Some(mut rx) = signaling_clone.take_signal_receiver().await {
                tokio::spawn(actor.run());
                while let Some(signal) = tokio::select! {
                    _ = signal_tx.closed() => None,
                    _ = signaling_clone.closed() => None,
                    signal = rx.recv() => signal,
                } {
                    if signal_tx.send(signal).await.is_err() {
                        break;
                    }
                }
            }
        });

        Self { request_tx }
    }

    /// Dial connects to a remote network.
    ///
    /// Returns a `NetherNetStream` on success, or an error if connection fails.
    #[inline]
    pub async fn dial(&self, network_id: String) -> Result<NetherNetStream, NetherNetError> {
        let (tx, rx) = oneshot::channel();
        self.request_tx
            .send(DialerRequest::Dial {
                network_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetherNetError::ConnectionClosed)?;

        rx.await.map_err(|_| NetherNetError::ConnectionClosed)?
    }
}

impl DialerActor {
    async fn run(mut self) {
        let mut requests_closed = false;
        let mut cleanup = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = self.signaling.closed() => break,
                signal = self.signal_rx.recv() => {
                    let Some(signal) = signal else { break };
                    self.dispatch_signal(signal).await;
                }
                request = self.request_rx.recv(), if !requests_closed => {
                    match request {
                        Some(request) => self.handle_request(request).await,
                        None => requests_closed = true,
                    }
                }
                _ = cleanup.tick() => {
                    self.pending_dials.retain(|_, tx| !tx.is_closed());
                    if requests_closed && self.pending_dials.is_empty() { break; }
                }
            }
        }

        debug!("Dialer actor shutting down");
    }

    async fn dispatch_signal(&mut self, signal: Signal) {
        if signal.validate().is_err() || signal.data.len() > 1024 * 1024 {
            return;
        }
        let conn_id = signal.connection_id;
        let key = (signal.network_id.clone(), conn_id);
        if let Some(tx) = self.pending_dials.get_mut(&key) {
            // Never block the dispatcher behind a single peer's full queue.
            // Upstream dial.go:signalSubscription.NotifySignal is also non-blocking.
            if tx.try_send(signal).is_err() {
                self.pending_dials.remove(&key);
                trace!(conn_id, "Closing congested or closed signal subscription");
            }
        } else {
            trace!(conn_id, "Signal for unknown connection");
        }
    }

    fn local_identity(&mut self) -> Result<LocalIdentity, NetherNetError> {
        if let Some(local) = &self.local {
            return Ok(local.clone());
        }
        let local = LocalIdentity::generate()?;
        self.local = Some(local.clone());
        Ok(local)
    }

    async fn handle_request(&mut self, request: DialerRequest) {
        match request {
            DialerRequest::Dial {
                network_id,
                response_tx,
            } => {
                self.pending_dials.retain(|_, tx| !tx.is_closed());
                // Local admission budget, matching listener.go:maxListenerNegotiations.
                if self.pending_dials.len() >= 64 {
                    let _ = response_tx.send(Err(NetherNetError::ConnectionClosed));
                    return;
                }
                let local = match self.local_identity() {
                    Ok(local) => local,
                    Err(error) => {
                        let _ = response_tx.send(Err(error));
                        return;
                    }
                };
                let mut connection_id = rand::random::<u64>();
                while self
                    .pending_dials
                    .contains_key(&(network_id.clone(), connection_id))
                {
                    connection_id = rand::random();
                }
                let (signal_in_tx, signal_in_rx) = mpsc::channel(32);

                self.pending_dials
                    .insert((network_id.clone(), connection_id), signal_in_tx);

                let signaling = self.signaling.clone();
                let config = self.config.clone();

                tokio::spawn(async move {
                    let dial = perform_dial(
                        connection_id,
                        network_id,
                        signaling,
                        config,
                        local,
                        signal_in_rx,
                    );
                    // Report a contained panic instead of dropping response_tx,
                    // which the caller would otherwise see as ConnectionClosed.
                    let result = catch_dependency_panic("dial negotiation", dial)
                        .await
                        .and_then(|result| result);
                    let _ = response_tx.send(result);
                });
            }
        }
    }
}

/// Explicit ICE configuration is the static equivalent of upstream
/// dial.go:Dialer.DialContext's Credentials override. Otherwise use signaling's
/// fresh TURN credentials through credentials.go:gatherOptions.
pub(crate) async fn ice_servers(
    signaling: &dyn Signaling,
    configured: &[IceServer],
) -> Result<Vec<IceServer>, NetherNetError> {
    if !configured.is_empty() {
        return Ok(configured.to_vec());
    }
    Ok(signaling
        .credentials()
        .await
        .map_err(NetherNetError::Signaling)?
        .map(|credentials| credentials.ice_servers)
        .unwrap_or_default())
}

/// Local resource bound for conn.go:addRemoteCandidate's peer-controlled state.
/// Inline SDP and trickled candidates share this budget; it is not a wire constant.
const MAX_REMOTE_CANDIDATES: usize = 128;

#[derive(Default)]
pub(crate) struct CandidateBudget(std::sync::atomic::AtomicUsize);

impl CandidateBudget {
    fn reserve(&self, count: usize) -> Result<(), NetherNetError> {
        self.0
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |used| {
                    used.checked_add(count)
                        .filter(|&total| total <= MAX_REMOTE_CANDIDATES)
                },
            )
            .map(|_| ())
            .map_err(|_| NetherNetError::TooManyIceCandidates {
                limit: MAX_REMOTE_CANDIDATES,
            })
    }

    pub(crate) fn reserve_sdp(&self, sdp: &str) -> Result<(), NetherNetError> {
        self.reserve(
            sdp.lines()
                .filter(|line| {
                    line.strip_prefix("a=")
                        .is_some_and(|value| value.trim_start().starts_with("candidate:"))
                })
                .count(),
        )
    }
}

/// Validate and add one trickled candidate; upstream conn.go:parseRemoteCandidate
/// accepts either `candidate:` or `a=candidate:`.
pub(crate) fn add_candidate(
    connection: &Connection,
    data: &str,
    budget: &CandidateBudget,
) -> Result<(), NetherNetError> {
    let candidate = data.trim().strip_prefix("a=").unwrap_or(data.trim());
    parse_ice_candidate(candidate).map_err(|_| NetherNetError::InvalidIceCandidate)?;
    budget.reserve(1)?;
    connection.add_remote_candidate(candidate)
}

/// Signal each newly gathered local candidate (conn.go:trickleCandidates);
/// a failed signal closes the connection, as upstream does.
pub(crate) fn trickle_candidates(
    connection: Arc<Connection>,
    signaling: Arc<dyn Signaling>,
    remote: String,
    connection_id: u64,
) {
    tokio::spawn(async move {
        let ufrag = connection.local_ufrag();
        let mut network_id = 0u32;
        while let Some(candidates) = connection.next_candidates().await {
            for candidate in candidates {
                let data = candidate.format(network_id, &ufrag);
                network_id += 1;
                // conn.go:trickleCandidate bounds each signaling operation to 15 seconds.
                let sent = timeout(
                    Duration::from_secs(15),
                    signaling.signal(Signal {
                        typ: signal_type::CANDIDATE.into(),
                        connection_id,
                        data,
                        network_id: remote.clone(),
                    }),
                )
                .await;
                if !matches!(sent, Ok(Ok(()))) {
                    connection.close();
                    return;
                }
            }
        }
    });
}

/// Performs one negotiation under the single deadline used by upstream dial.go:Dialer.Dial.
async fn perform_dial(
    connection_id: u64,
    network_id: String,
    signaling: Arc<dyn Signaling>,
    config: NetherNetDialerConfig,
    local: LocalIdentity,
    mut signal_rx: mpsc::Receiver<Signal>,
) -> Result<NetherNetStream, NetherNetError> {
    config.stream_config.validate()?;
    let connection_timeout = config.connection_timeout;
    let negotiation = async {
        let servers = ice_servers(signaling.as_ref(), &config.ice_servers).await?;
        let pending = Pending::open(
            Role::Dialer,
            &local,
            &config.transport,
            &servers,
            &config.stream_config,
        )
        .await?;
        let nontrickle = signaling.disable_trickle_ice();
        if nontrickle {
            pending.gathered().await?;
        }
        let mut offer = pending.describe(sdp::Setup::ActPass, nontrickle)?;
        if let Some(identity) = &config.identity {
            offer = identity
                .sign_sdp(&offer)
                .map_err(|e| NetherNetError::Signaling(e.into()))?;
        }
        signaling
            .signal(Signal {
                typ: signal_type::OFFER.into(),
                connection_id,
                data: offer,
                network_id: network_id.clone(),
            })
            .await
            .map_err(NetherNetError::Signaling)?;
        // Trickle only after the offer (conn.go:trickleCandidates).
        if !nontrickle {
            trickle_candidates(
                pending.connection().clone(),
                signaling.clone(),
                network_id.clone(),
                connection_id,
            );
        }

        let mut answered = false;
        let mut pending_candidates: Vec<String> = Vec::new();
        let mut remote_identity = None;
        let candidate_budget = CandidateBudget::default();
        loop {
            tokio::select! {
                opened = pending.opened(), if answered => {
                    opened?;
                    break;
                }
                signal = signal_rx.recv() => {
                    let signal = signal.ok_or(NetherNetError::ConnectionClosed)?;
                    match signal.typ.as_str() {
                        signal_type::ANSWER if !answered => {
                            let allow_missing = config.allow_identityless_server ||
                                (!signaling.requires_identity() && config.verify_server_token.is_none());
                            remote_identity = crate::identity::verify_sdp_optional(
                                &signal.data, crate::identity::PeerRole::Server,
                                config.verify_server_token.as_deref(), allow_missing,
                            ).await.map_err(|e| NetherNetError::Signaling(e.into()))?;
                            candidate_budget.reserve_sdp(&signal.data)?;
                            let remote = sdp::parse(&signal.data)?;
                            pending.connection().with("answer", |session, now| {
                                session.start(now, &remote, sdp::Setup::ActPass)
                            })??;
                            for candidate in pending_candidates.drain(..) {
                                add_candidate(pending.connection(), &candidate, &candidate_budget)?;
                            }
                            answered = true;
                        }
                        signal_type::CANDIDATE => {
                            if answered {
                                add_candidate(pending.connection(), &signal.data, &candidate_budget)?;
                            } else if pending_candidates.len() < 32 {
                                pending_candidates.push(signal.data);
                            } else {
                                return Err(NetherNetError::InvalidPacket);
                            }
                        }
                        signal_type::ERROR => {
                            let code = crate::signaling::parse_signal_error_code(&signal.data)
                                .map_err(|e| NetherNetError::Signaling(e.into()))?;
                            return Err(NetherNetError::Signaling(anyhow::anyhow!("Remote connection error: {code}")));
                        }
                        _ => return Err(NetherNetError::InvalidPacket),
                    }
                }
            }
        }
        let connection = Arc::downgrade(pending.connection());
        let stream = pending.into_stream(remote_identity);
        // Keep routing post-establishment ICE/errors without retaining the connection.
        tokio::spawn(async move {
            loop {
                // Ended connections must release their bounded subscription even with no new signals.
                let signal = tokio::select! {
                    signal = signal_rx.recv() => signal,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if connection.upgrade().is_none_or(|c| c.is_finished()) { break; }
                        continue;
                    }
                };
                let (Some(signal), Some(connection)) = (signal, connection.upgrade()) else {
                    break;
                };
                match signal.typ.as_str() {
                    signal_type::CANDIDATE
                        if add_candidate(&connection, &signal.data, &candidate_budget).is_err() =>
                    {
                        connection.close();
                        break;
                    }
                    signal_type::ERROR => {
                        connection.close();
                        break;
                    }
                    _ => {}
                }
            }
        });
        Ok(stream)
    };
    match timeout(connection_timeout, negotiation).await {
        Ok(result) => result,
        Err(_) => {
            // dial.go:SignalErrorTimeout bounds terminal notification to two seconds.
            tokio::spawn(async move {
                let _ = timeout(
                    Duration::from_secs(2),
                    signaling.signal(Signal::error(
                        connection_id,
                        network_id,
                        SignalErrorCode::NegotiationTimeoutWaitingForResponse,
                    )),
                )
                .await;
            });
            Err(NetherNetError::NegotiationTimeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoSignaling;

    #[async_trait::async_trait]
    impl Signaling for NoSignaling {
        async fn signal(&self, _: Signal) -> anyhow::Result<()> {
            Ok(())
        }
        fn network_id(&self) -> String {
            "local".into()
        }
    }

    #[tokio::test]
    async fn rejects_signals_from_other_networks() {
        let (_, signal_rx) = mpsc::channel(1);
        let (_, request_rx) = mpsc::channel(1);
        let (tx, mut rx) = mpsc::channel(1);
        let mut actor = DialerActor {
            signaling: Arc::new(NoSignaling),
            config: NetherNetDialerConfig::default(),
            signal_rx,
            request_rx,
            pending_dials: std::collections::HashMap::from([(("remote".into(), 7), tx)]),
            local: None,
        };
        actor
            .dispatch_signal(Signal {
                typ: signal_type::ANSWER.into(),
                connection_id: 7,
                data: "forged answer".into(),
                network_id: "attacker".into(),
            })
            .await;
        assert!(
            rx.try_recv().is_err(),
            "foreign answer reached pending dial"
        );
    }

    #[test]
    fn inline_and_trickled_candidates_share_a_nonwrapping_budget() {
        let budget = CandidateBudget::default();
        budget.reserve(MAX_REMOTE_CANDIDATES - 2).unwrap();
        budget
            .reserve_sdp("a= candidate:one\r\na=candidate:two\r\n")
            .unwrap();
        assert!(matches!(
            budget.reserve(1),
            Err(NetherNetError::TooManyIceCandidates { limit: 128 })
        ));
        assert!(budget.reserve(usize::MAX).is_err());
        assert!(
            budget.reserve(0).is_ok(),
            "failed reservations must leave the budget valid"
        );
    }
}
