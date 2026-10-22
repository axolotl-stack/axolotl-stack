use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use tracing::{debug, trace};
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::dtls::extension::extension_use_srtp::SrtpProtectionProfile;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use crate::error::{NetherNetError, catch_dependency_panic};
use crate::signaling::{
    Signal, SignalErrorCode, SignalType, Signaling, format_ice_candidate, parse_ice_candidate,
};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};

/// Configuration for `NetherNetDialer`.
#[derive(Clone)]
pub struct NetherNetDialerConfig {
    /// Configuration for streams created by this dialer.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// Explicit ICE servers; nonempty values override signaling credentials.
    pub ice_servers: Vec<RTCIceServer>,
    /// Optional WebRTC settings for explicit local interface/ICE policy.
    /// `None` preserves webrtc-rs defaults, including excluding loopback candidates.
    /// SRTP protection profiles are always replaced by `SRTP_PROTECTION_PROFILES`.
    pub setting_engine: Option<SettingEngine>,
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
            setting_engine: None,
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
                    let dial =
                        perform_dial(connection_id, network_id, signaling, config, signal_in_rx);
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
    configured: &[RTCIceServer],
) -> Result<Vec<RTCIceServer>, NetherNetError> {
    if !configured.is_empty() {
        return Ok(configured.to_vec());
    }
    Ok(
        match signaling
            .credentials()
            .await
            .map_err(NetherNetError::Signaling)?
        {
            Some(credentials) => credentials
                .ice_servers
                .into_iter()
                .map(|server| RTCIceServer {
                    urls: server.urls,
                    username: server.username,
                    credential: server.password,
                })
                .collect(),
            None => configured.to_vec(),
        },
    )
}

/// SRTP profiles that webrtc-srtp 0.14 can actually key.
///
/// webrtc-rs 0.12 `dtls_transport/mod.rs:default_srtp_protection_profiles` also
/// offers SRTP_AEAD_AES_256_GCM, but webrtc-srtp 0.14
/// `cipher/cipher_aead_aes_gcm.rs:CipherAeadAesGcm::new` and
/// `key_derivation.rs:aes_cm_key_derivation` hard-code AES-128 and panic
/// (generic-array length 32 != 16) on that profile's 32-byte master key, inside
/// webrtc-rs's detached operation task, so SCTP never starts. Pion v4
/// (`constants.go:defaultSrtpProtectionProfiles`, used by df-mc/go-nethernet)
/// lists AES-256-GCM first; pion/dtls v3 servers select by their own preference
/// (`flight0handler.go`) and webrtc-dtls servers by the client's (`flight0.rs`),
/// so pairings with go-nethernet negotiated the unimplemented profile in both
/// directions. NetherNet carries no media: the profile only has to be keyable,
/// and pion supports all of these.
const SRTP_PROTECTION_PROFILES: [SrtpProtectionProfile; 3] = [
    SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
    SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80,
    SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_32,
];

/// Caller settings (or webrtc-rs defaults) restricted to keyable SRTP profiles.
pub(crate) fn nethernet_setting_engine(settings: Option<SettingEngine>) -> SettingEngine {
    let mut settings = settings.unwrap_or_default();
    settings.set_srtp_protection_profiles(SRTP_PROTECTION_PROFILES.to_vec());
    settings
}

/// Close transports when negotiation is cancelled or any fallible step fails.
pub(crate) struct PeerGuard(pub Option<Arc<webrtc::peer_connection::RTCPeerConnection>>);

impl Drop for PeerGuard {
    fn drop(&mut self) {
        if let Some(pc) = self.0.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                if let Err(error) = pc.close().await {
                    debug!(%error, "Closing peer transports failed");
                }
            });
        }
    }
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

/// Add the original candidate, preserving related address, TCP type and extensions.
/// Upstream conn.go:parseRemoteCandidate accepts either candidate: or a=candidate:.
pub(crate) async fn add_candidate(
    pc: &webrtc::peer_connection::RTCPeerConnection,
    data: &str,
    budget: &CandidateBudget,
) -> Result<(), NetherNetError> {
    let candidate = data.trim().strip_prefix("a=").unwrap_or(data.trim());
    let info = parse_ice_candidate(candidate).map_err(|_| NetherNetError::InvalidIceCandidate)?;
    budget.reserve(1)?;
    pc.add_ice_candidate(webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
        candidate: candidate.to_owned(),
        sdp_mid: Some("0".into()),
        sdp_mline_index: Some(0),
        username_fragment: info.ufrag.map(str::to_owned),
    })
    .await?;
    Ok(())
}

/// Performs one negotiation under the single deadline used by upstream dial.go:Dialer.Dial.
async fn perform_dial(
    connection_id: u64,
    network_id: String,
    signaling: Arc<dyn Signaling>,
    mut config: NetherNetDialerConfig,
    mut signal_rx: mpsc::Receiver<Signal>,
) -> Result<NetherNetStream, NetherNetError> {
    config.stream_config.validate()?;
    let connection_timeout = config.connection_timeout;
    let negotiation = async {
        let mut media_engine = MediaEngine::default();
        let registry = register_default_interceptors(Registry::new(), &mut media_engine)?;
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(nethernet_setting_engine(config.setting_engine.take()))
            .build();
        let pc = Arc::new(
            api.new_peer_connection(RTCConfiguration {
                ice_servers: ice_servers(signaling.as_ref(), &config.ice_servers).await?,
                ..Default::default()
            })
            .await?,
        );
        let mut guard = PeerGuard(Some(pc.clone()));
        let weak_pc = Arc::downgrade(&pc);
        pc.on_data_channel(Box::new(move |_| {
            let weak_pc = weak_pc.clone();
            Box::pin(async move {
                // dial.go:Dialer.DialContext rejects server-opened channels.
                if let Some(pc) = weak_pc.upgrade() {
                    tokio::spawn(async move {
                        let _ = pc.close().await;
                    });
                }
            })
        }));

        // message.go:MessageReliability.Parameters specifies both channel labels and ordering.
        let reliable = pc.create_data_channel("ReliableDataChannel", None).await?;
        let unreliable = pc
            .create_data_channel(
                "UnreliableDataChannel",
                Some(RTCDataChannelInit {
                    ordered: Some(false),
                    max_retransmits: Some(0),
                    ..Default::default()
                }),
            )
            .await?;
        // Register before any SDP can start transports: an eager peer may send on open.
        let (incoming_tx, incoming_rx) =
            mpsc::channel(config.stream_config.incoming_channel_capacity);
        crate::stream::install_message_handler(&reliable, incoming_tx.clone(), true);
        crate::stream::install_message_handler(&unreliable, incoming_tx.clone(), false);
        let (opened_tx, mut opened_rx) = mpsc::channel(2);
        for channel in [&reliable, &unreliable] {
            let tx = opened_tx.clone();
            channel.on_open(Box::new(move || {
                let tx = tx.clone();
                Box::pin(async move {
                    let _ = tx.send(()).await;
                })
            }));
        }
        drop(opened_tx);

        let offer = pc.create_offer(None).await?;
        let mut offer_sdp = crate::stream::local_sdp(offer.sdp.clone());
        if let Some(identity) = &config.identity {
            offer_sdp = identity
                .sign_sdp(&offer_sdp)
                .map_err(|e| NetherNetError::Signaling(e.into()))?;
        }
        let ufrag = offer_sdp
            .lines()
            .find_map(|line| line.strip_prefix("a=ice-ufrag:"))
            .unwrap_or_default()
            .to_owned();
        let sig = signaling.clone();
        let remote = network_id.clone();
        let candidate_id = Arc::new(std::sync::atomic::AtomicU32::new(0));
        // Gate trickle messages until the offer is sent (conn.go:trickleCandidates).
        let (started_tx, started_rx) = tokio::sync::watch::channel(false);
        let weak_pc = Arc::downgrade(&pc);
        let nontrickle = signaling.disable_trickle_ice();
        if !nontrickle {
            pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
                let sig = sig.clone();
                let remote = remote.clone();
                let ufrag = ufrag.clone();
                let mut started = started_rx.clone();
                let weak_pc = weak_pc.clone();
                let id = candidate_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Box::pin(async move {
                    let Some(candidate) = candidate else { return };
                    if started.wait_for(|ready| *ready).await.is_err() {
                        return;
                    }
                    let data = format_ice_candidate(
                        id,
                        &candidate.foundation,
                        &candidate.protocol.to_string().to_lowercase(),
                        candidate.priority,
                        &candidate.address,
                        candidate.port,
                        &candidate.typ.to_string().to_lowercase(),
                        (!candidate.related_address.is_empty())
                            .then_some(candidate.related_address.as_str()),
                        (candidate.related_port != 0).then_some(candidate.related_port),
                        &ufrag,
                    );
                    // conn.go:trickleCandidate bounds each signaling operation to 15 seconds.
                    if !matches!(
                        timeout(
                            Duration::from_secs(15),
                            sig.signal(Signal {
                                typ: SignalType::Candidate,
                                connection_id,
                                data,
                                network_id: remote,
                            })
                        )
                        .await,
                        Ok(Ok(()))
                    ) && let Some(pc) = weak_pc.upgrade()
                    {
                        let _ = pc.close().await;
                    }
                })
            }));
        }
        let mut gathering = pc.gathering_complete_promise().await;
        // webrtc-rs validates local SDP against its exact generated offer.
        // Apply the native description internally; normalize/sign only the wire SDP.
        pc.set_local_description(offer).await?;
        if nontrickle {
            gathering.recv().await;
            offer_sdp = crate::stream::local_sdp(
                pc.local_description()
                    .await
                    .ok_or(NetherNetError::ConnectionClosed)?
                    .sdp,
            );
            if let Some(identity) = &config.identity {
                offer_sdp = identity
                    .sign_sdp(&offer_sdp)
                    .map_err(|e| NetherNetError::Signaling(e.into()))?;
            }
        }
        signaling
            .signal(Signal {
                typ: SignalType::Offer,
                connection_id,
                data: offer_sdp,
                network_id: network_id.clone(),
            })
            .await
            .map_err(NetherNetError::Signaling)?;
        let _ = started_tx.send(true);

        let mut answered = false;
        let mut opened = 0;
        let mut pending_candidates: Vec<String> = Vec::new();
        let mut remote_identity = None;
        let candidate_budget = CandidateBudget::default();
        while !answered || opened < 2 {
            tokio::select! {
                event = opened_rx.recv(), if opened < 2 => {
                    event.ok_or(NetherNetError::ConnectionClosed)?;
                    opened += 1;
                }
                signal = signal_rx.recv() => {
                    let signal = signal.ok_or(NetherNetError::ConnectionClosed)?;
                    match signal.typ {
                        SignalType::Answer if !answered => {
                            let allow_missing = config.allow_identityless_server ||
                                (!signaling.requires_identity() && config.verify_server_token.is_none());
                            remote_identity = crate::identity::verify_sdp_optional(
                                &signal.data, crate::identity::PeerRole::Server,
                                config.verify_server_token.as_deref(), allow_missing,
                            ).await.map_err(|e| NetherNetError::Signaling(e.into()))?;
                            candidate_budget.reserve_sdp(&signal.data)?;
                            config.stream_config.max_message_size = crate::stream::negotiate_max_message_size(
                                &signal.data, config.stream_config.max_message_size)?;
                            pc.set_remote_description(RTCSessionDescription::answer(signal.data)?).await?;
                            for candidate in pending_candidates.drain(..) {
                                add_candidate(&pc, &candidate, &candidate_budget).await?;
                            }
                            answered = true;
                        }
                        SignalType::Candidate => {
                            if answered {
                                add_candidate(&pc, &signal.data, &candidate_budget).await?;
                            } else if pending_candidates.len() < 32 {
                                pending_candidates.push(signal.data);
                            } else {
                                return Err(NetherNetError::InvalidPacket);
                            }
                        }
                        SignalType::Error => {
                            let code = crate::signaling::parse_signal_error_code(&signal.data)
                                .map_err(|e| NetherNetError::Signaling(e.into()))?;
                            return Err(NetherNetError::Signaling(anyhow::anyhow!("Remote connection error: {code}")));
                        }
                        _ => return Err(NetherNetError::InvalidPacket),
                    }
                }
            }
        }
        let mut stream = NetherNetStream::new_with_receiver(
            reliable,
            unreliable,
            incoming_tx,
            incoming_rx,
            config.stream_config,
        );
        stream.attach_peer(pc.clone());
        stream.set_remote_identity(remote_identity);
        guard.0 = None;
        // Keep routing post-establishment ICE/errors without retaining the peer forever.
        let weak_pc = Arc::downgrade(&pc);
        tokio::spawn(async move {
            loop {
                // Closed peers must release their bounded subscription even with no new signals.
                let signal = tokio::select! {
                    signal = signal_rx.recv() => signal,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if weak_pc.upgrade().is_none_or(|pc| pc.connection_state() == webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Closed) { break; }
                        continue;
                    }
                };
                let (Some(signal), Some(pc)) = (signal, weak_pc.upgrade()) else {
                    break;
                };
                match signal.typ {
                    SignalType::Candidate
                        if add_candidate(&pc, &signal.data, &candidate_budget)
                            .await
                            .is_err() =>
                    {
                        let _ = pc.close().await;
                        break;
                    }
                    SignalType::Error => {
                        let _ = pc.close().await;
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
        };
        actor
            .dispatch_signal(Signal {
                typ: SignalType::Answer,
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
