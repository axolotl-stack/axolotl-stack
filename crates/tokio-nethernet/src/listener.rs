use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use tracing::{debug, trace, warn};

use crate::error::catch_dependency_panic;
use crate::signaling::{IceServer, Signal, SignalErrorCode, SignalType, Signaling};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};
use crate::transport::session::Role;
use crate::transport::{Connection, LocalIdentity, Pending, TransportConfig, sdp};

/// Configuration for `NetherNetListener`.
#[derive(Clone)]
pub struct NetherNetListenerConfig {
    /// Configuration for streams created by this listener.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// Explicit ICE servers; nonempty values override signaling credentials.
    pub ice_servers: Vec<IceServer>,
    /// Local interface/ICE policy; the default matches go-nethernet's pion
    /// defaults (all non-loopback UDP4/UDP6 addresses).
    pub transport: TransportConfig,
    /// Maximum SDP size in bytes (prevents OOM from large SDPs).
    pub max_sdp_size: usize,
    /// Maximum tracked negotiations/connections (local resource hardening).
    pub max_pending_connections: usize,
    /// Local server SDP identity; absent values use the upstream-generated self identity.
    pub identity: Option<Arc<crate::identity::Identity>>,
    /// Optional client-token authorization; otherwise Login must authenticate and bind its cpk.
    pub verify_client_token: Option<Arc<dyn crate::identity::TokenVerifier>>,
    /// Explicit local/testing opt-out; upstream rejects identityless clients by default.
    pub allow_identityless_client: bool,
    /// Optional local anomaly tracking; does not alter wire behavior.
    pub signal_monitor: Option<Arc<crate::signal_monitor::SignalMonitor>>,
}

/// Local hardening budget for SDP parsing; not a NetherNet wire constant.
const DEFAULT_MAX_SDP_SIZE: usize = 64 * 1024;

/// df-mc/go-nethernet listener.go:maxListenerNegotiations.
const DEFAULT_MAX_PENDING_CONNECTIONS: usize = 64;

impl Default for NetherNetListenerConfig {
    fn default() -> Self {
        Self {
            stream_config: NetherNetStreamConfig::default(),
            // The real client's host has no negotiation deadline (only ICE
            // failure ends a session) and its dialer gives up 10-15 s after
            // CONNECTREQUEST; go-nethernet's 5 s refused slow vanilla peers.
            // 15 s keeps a local bound that never undercuts the client.
            connection_timeout: Duration::from_secs(15),
            // credentials.go:gatherOptions uses host candidates when credentials are absent.
            ice_servers: Vec::new(),
            transport: TransportConfig::default(),
            max_sdp_size: DEFAULT_MAX_SDP_SIZE,
            max_pending_connections: DEFAULT_MAX_PENDING_CONNECTIONS,
            identity: None,
            verify_client_token: None,
            allow_identityless_client: false,
            signal_monitor: None,
        }
    }
}

/// A listener for incoming NetherNet connections.
///
/// Similar to `RaknetListener`, this spawns a background actor to handle
/// signaling and connection negotiation. Use `accept()` to receive established streams.
pub struct NetherNetListener {
    /// Channel for accepted streams.
    accept_rx: mpsc::Receiver<NetherNetStream>,
}

/// Internal message for cleanup coordination.
enum ActorMessage {
    Signal(Signal),
    CleanupConnection((String, u64), Arc<Connection>),
}

struct ListenerActor {
    signaling: Arc<dyn Signaling>,
    config: NetherNetListenerConfig,
    message_rx: mpsc::Receiver<ActorMessage>,
    accept_tx: mpsc::Sender<NetherNetStream>,
    connections: HashMap<(String, u64), Arc<ConnectionHandle>>,
    /// Channel for spawned tasks to request cleanup.
    cleanup_tx: mpsc::Sender<ActorMessage>,
    generated_identity_key: Option<p384::ecdsa::SigningKey>,
    /// DTLS certificate shared by this listener's connections (generated on first offer).
    local: Option<LocalIdentity>,
}

struct ConnectionHandle {
    connection: Arc<Connection>,
    candidate_budget: crate::dialer::CandidateBudget,
    cancelled: tokio_util::sync::CancellationToken,
}

impl NetherNetListener {
    /// Creates a new listener with the given signaling implementation.
    ///
    /// Returns the listener and a channel sender for dispatching received signals.
    /// The caller is responsible for routing incoming network signals to this sender.
    ///
    /// # Background Task
    /// This spawns a background tokio task (the `ListenerActor`) that handles:
    /// - Processing incoming `CONNECTREQUEST` signals
    /// - ICE candidate exchange
    /// - Connection establishment and stream creation
    pub fn new(
        signaling: Arc<dyn Signaling>,
        config: NetherNetListenerConfig,
    ) -> (Self, mpsc::Sender<Signal>) {
        let (accept_tx, accept_rx) = mpsc::channel(16);
        let (signal_tx, mut signal_rx) = mpsc::channel::<Signal>(128);
        let (message_tx, message_rx) = mpsc::channel::<ActorMessage>(256);

        // Forward signals to actor message channel
        let message_tx_clone = message_tx.clone();
        tokio::spawn(async move {
            while let Some(signal) = signal_rx.recv().await {
                if message_tx_clone
                    .send(ActorMessage::Signal(signal))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        let actor = ListenerActor {
            signaling,
            config,
            message_rx,
            accept_tx,
            connections: HashMap::new(),
            cleanup_tx: message_tx,
            generated_identity_key: None,
            local: None,
        };

        tokio::spawn(actor.run());

        (Self { accept_rx }, signal_tx)
    }

    /// Binds a listener using a signaling channel that implements Stream + Sink.
    ///
    /// This is the preferred constructor for ergonomic usage - it internalizes
    /// the signal routing, eliminating boilerplate in user code.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let discovery = DiscoveryListener::bind("0.0.0.0:7551", config).await?;
    /// let mut listener = NetherNetListener::bind_with_signaling(
    ///     discovery,
    ///     NetherNetListenerConfig::default()
    /// );
    /// while let Ok(stream) = listener.accept().await {
    ///     // handle stream
    /// }
    /// ```
    pub fn bind_with_signaling<S>(signaling: S, config: NetherNetListenerConfig) -> Self
    where
        S: crate::signaling::SignalingChannel + 'static,
    {
        let (accept_tx, accept_rx) = mpsc::channel(16);
        let (message_tx, message_rx) = mpsc::channel::<ActorMessage>(256);

        // Create actor with Arc<S> as the Signaling impl
        let signaling = std::sync::Arc::new(signaling);
        let signaling_clone = signaling.clone();

        let actor = ListenerActor {
            signaling: signaling.clone(),
            config,
            message_rx,
            accept_tx,
            connections: HashMap::new(),
            cleanup_tx: message_tx.clone(),
            generated_identity_key: None,
            local: None,
        };

        // Install subscription before processing offers.
        tokio::spawn(async move {
            if let Some(mut rx) = signaling_clone.take_signal_receiver().await {
                tokio::spawn(actor.run());
                while let Some(signal) = tokio::select! {
                    _ = message_tx.closed() => None,
                    _ = signaling_clone.closed() => None,
                    signal = rx.recv() => signal,
                } {
                    if message_tx.send(ActorMessage::Signal(signal)).await.is_err() {
                        break;
                    }
                }
            }
        });

        Self { accept_rx }
    }

    /// Accepts the next incoming connection.
    ///
    /// Returns `Err(ConnectionClosed)` if the listener has been shut down.
    #[inline]
    pub async fn accept(&mut self) -> Result<NetherNetStream, crate::NetherNetError> {
        self.accept_rx
            .recv()
            .await
            .ok_or(crate::NetherNetError::ConnectionClosed)
    }

    /// Poll-based accept for use with manual polling or futures combinators.
    pub fn poll_accept(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<NetherNetStream>> {
        self.accept_rx.poll_recv(cx)
    }
}

impl ListenerActor {
    async fn run(mut self) {
        loop {
            let message = tokio::select! {
                _ = self.accept_tx.closed() => break,
                _ = self.signaling.closed() => break,
                message = self.message_rx.recv() => match message {
                    Some(message) => message,
                    None => break,
                },
            };
            match message {
                ActorMessage::Signal(signal) => {
                    trace!(typ = %signal.typ, conn_id = signal.connection_id, "Processing signal");

                    if let Err(e) = self.handle_signal(signal).await {
                        warn!(error = %e, "Error processing signal");
                    }
                }
                ActorMessage::CleanupConnection(key, connection) => {
                    self.cleanup_connection(&key, &connection)
                }
            }
        }

        for handle in self.connections.into_values() {
            handle.cancelled.cancel();
            handle.connection.close();
        }
        debug!("Listener actor shutting down");
    }

    fn cleanup_connection(&mut self, key: &(String, u64), connection: &Arc<Connection>) {
        // listener.go:listenerNegotiator.unregister must not remove a replacement owner.
        if self
            .connections
            .get(key)
            .is_some_and(|handle| Arc::ptr_eq(&handle.connection, connection))
        {
            self.connections.remove(key);
        }
    }

    async fn handle_signal(&mut self, signal: Signal) -> anyhow::Result<()> {
        signal.validate()?;
        if let Some(monitor) = &self.config.signal_monitor {
            monitor.process_incoming(&signal).await;
        }
        let conn_id = signal.connection_id;
        // listener.go:negotiationKey isolates identical connection IDs on different networks.
        let key = (signal.network_id.clone(), conn_id);

        match signal.typ {
            SignalType::Offer => {
                if self.connections.contains_key(&key) {
                    debug!(conn_id, "Ignoring duplicate CONNECTREQUEST");
                    return Ok(());
                }
                self.handle_connect_request(signal)?;
            }
            SignalType::Candidate => {
                if let Some(handle) = self.connections.get(&key) {
                    if let Err(error) = crate::dialer::add_candidate(
                        &handle.connection,
                        &signal.data,
                        &handle.candidate_budget,
                    ) {
                        // Panics are contained by the transport and fail only this connection.
                        if handle.connection.is_finished() {
                            handle.connection.close();
                        }
                        return Err(error.into());
                    }
                } else {
                    debug!(conn_id, "Received CANDIDATE for unknown connection");
                }
            }
            SignalType::Error => {
                warn!(conn_id, data = %signal.data, from = %signal.network_id, "Received CONNECTERROR");
                if let Some(handle) = self.connections.get(&key) {
                    // Keep tuple ownership until its cancelled worker has stopped replying.
                    handle.cancelled.cancel();
                    handle.connection.close();
                }
            }
            _ => {
                debug!(typ = %signal.typ, "Unknown signal type");
            }
        }

        Ok(())
    }

    fn local_identity(&mut self) -> Result<LocalIdentity, crate::NetherNetError> {
        if let Some(local) = &self.local {
            return Ok(local.clone());
        }
        let local = LocalIdentity::generate()?;
        self.local = Some(local.clone());
        Ok(local)
    }

    /// Sends a terminal `CONNECTERROR` without blocking the actor.
    fn reject(&self, conn_id: u64, network_id: String, code: SignalErrorCode) {
        tokio::spawn(send_terminal_error(
            self.signaling.clone(),
            conn_id,
            network_id,
            code,
        ));
    }

    /// Admits an offer and starts its negotiation task.
    ///
    /// Only cheap, local work runs here (limits, SDP parsing, opening the
    /// session). Everything that can wait on a peer or an integration hook
    /// (identity verification, credentials, candidate gathering, sending the
    /// answer) runs in the offer's own task, so one slow offer never delays
    /// signals for other peers.
    fn handle_connect_request(&mut self, signal: Signal) -> anyhow::Result<()> {
        let conn_id = signal.connection_id;
        let network_id = signal.network_id.clone();
        self.config.stream_config.validate()?;

        // Security: Check connection limit
        let negotiating = self.connections.len();
        if negotiating >= self.config.max_pending_connections {
            warn!(
                conn_id,
                current = negotiating,
                max = self.config.max_pending_connections,
                "Connection limit reached, rejecting new connection"
            );
            self.reject(
                conn_id,
                network_id,
                SignalErrorCode::IncomingConnectionIgnored,
            );
            return Ok(());
        }

        // Security: Check SDP size
        if signal.data.len() > self.config.max_sdp_size {
            warn!(
                conn_id,
                sdp_len = signal.data.len(),
                max = self.config.max_sdp_size,
                "SDP exceeds maximum size"
            );
            self.reject(
                conn_id,
                network_id,
                SignalErrorCode::SignalingParsingFailure,
            );
            return Ok(());
        }
        let remote = sdp::parse(&signal.data)?;
        let candidate_budget = crate::dialer::CandidateBudget::default();
        candidate_budget.reserve_sdp(&signal.data)?;
        let allow_missing = self.config.allow_identityless_client
            || (!self.signaling.requires_identity() && self.config.verify_client_token.is_none());
        // listener.go:ListenConfig.Listen defaults to one P-384 key per listener
        // and issues a fresh self-signed server token for each negotiation.
        let server_identity = if let Some(identity) = &self.config.identity {
            Some(identity.clone())
        } else if self.signaling.requires_identity() {
            let key = self.generated_identity_key.get_or_insert_with(|| {
                p384::ecdsa::SigningKey::random(&mut p384::elliptic_curve::rand_core::OsRng)
            });
            Some(Arc::new(crate::identity::Identity::generate_server(
                key.clone(),
                "self",
            )?))
        } else {
            None
        };

        let local = self.local_identity()?;
        let pending = Pending::open(
            Role::Listener,
            &local,
            &self.config.transport,
            &self.config.stream_config,
        )?;
        // listener.go:listenerNegotiator.answererRole.
        let local_setup = sdp::Setup::answer_to(remote.setup);
        // Inline offer candidates are present, so ICE starts immediately.
        pending.connection().with("offer", |session, now| {
            session.start(now, &remote, local_setup)
        })??;

        // A tuple remains owned until its worker/reply has finished or been cancelled.
        let cancelled = tokio_util::sync::CancellationToken::new();
        let connection = pending.connection().clone();
        let key = (network_id.clone(), conn_id);
        self.connections.insert(
            key.clone(),
            Arc::new(ConnectionHandle {
                connection: connection.clone(),
                candidate_budget,
                cancelled: cancelled.clone(),
            }),
        );

        let offer = Offer {
            signaling: self.signaling.clone(),
            sdp: signal.data,
            conn_id,
            network_id,
            local_setup,
            allow_missing,
            verify_client_token: self.config.verify_client_token.clone(),
            server_identity,
            ice_servers: self.config.ice_servers.clone(),
            signal_monitor: self.config.signal_monitor.clone(),
            accept_tx: self.accept_tx.clone(),
            connection_timeout: self.config.connection_timeout,
        };
        let cleanup_tx = self.cleanup_tx.clone();
        tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => {}
                _ = offer.negotiate(pending) => {}
            }

            // Release connection ownership only after failure or transport closure.
            let _ = cleanup_tx
                .send(ActorMessage::CleanupConnection(key, connection))
                .await;
        });

        Ok(())
    }
}

/// Bounded like listener.go:listenerNegotiator.signalError; dial.go:SignalErrorTimeout.
const TERMINAL_SIGNAL_TIMEOUT: Duration = Duration::from_secs(2);

async fn send_terminal_error(
    signaling: Arc<dyn Signaling>,
    conn_id: u64,
    network_id: String,
    code: SignalErrorCode,
) {
    let _ = timeout(
        TERMINAL_SIGNAL_TIMEOUT,
        signaling.signal(Signal::error(conn_id, network_id, code)),
    )
    .await;
}

/// Why answering an admitted offer failed.
enum AnswerError {
    /// The client identity was missing or not allowed.
    Identity(anyhow::Error),
    /// Credentials, gathering, signing or sending the answer failed.
    Other(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for AnswerError {
    fn from(error: E) -> Self {
        Self::Other(error.into())
    }
}

/// One admitted offer's negotiation, run in its own task.
struct Offer {
    signaling: Arc<dyn Signaling>,
    sdp: String,
    conn_id: u64,
    network_id: String,
    local_setup: sdp::Setup,
    allow_missing: bool,
    verify_client_token: Option<Arc<dyn crate::identity::TokenVerifier>>,
    server_identity: Option<Arc<crate::identity::Identity>>,
    ice_servers: Vec<IceServer>,
    signal_monitor: Option<Arc<crate::signal_monitor::SignalMonitor>>,
    accept_tx: mpsc::Sender<NetherNetStream>,
    connection_timeout: Duration,
}

impl Offer {
    /// Answers the offer, then waits for the data channels and hands the
    /// stream to `accept()`. Every failure before the stream is handed over
    /// replies one terminal `CONNECTERROR`; a remote `CONNECTERROR` cancels
    /// this future (and so any reply) through the handle's token.
    async fn negotiate(self, pending: Pending) {
        let conn_id = self.conn_id;
        // The answer phase shares the listener's negotiation deadline, so a
        // stalled credential provider or token verifier ends with a reply
        // instead of leaving the dialer to its own timeout.
        let answered = catch_dependency_panic(
            "listener offer",
            timeout(self.connection_timeout, self.answer(&pending)),
        )
        .await;
        let remote_identity = match answered {
            Ok(Ok(Ok(identity))) => identity,
            Ok(Ok(Err(AnswerError::Identity(error)))) => {
                // The real client's host and go-nethernet listener.go reply 37
                // instead of letting the dialer run into its 10-15 s answer timeout.
                warn!(conn_id, %error, "Rejecting offer: client identity not allowed");
                self.fail(SignalErrorCode::IdentityNotAllowed).await;
                return;
            }
            Ok(Ok(Err(AnswerError::Other(error)))) => {
                warn!(conn_id, %error, "Failed to answer offer");
                self.fail(SignalErrorCode::FailedToCreateAnswer).await;
                return;
            }
            Ok(Err(_)) => {
                warn!(conn_id, "Timed out answering offer");
                self.fail(SignalErrorCode::NegotiationTimeout).await;
                return;
            }
            Err(panic) => {
                warn!(conn_id, error = %panic, "Offer negotiation panicked");
                self.fail(SignalErrorCode::GenericFailure).await;
                return;
            }
        };

        let opened = timeout(self.connection_timeout, pending.opened()).await;
        match opened {
            Ok(Ok(())) => {
                debug!(conn_id, "Data channels ready");
                let stream = pending.into_stream(remote_identity);
                let weak = Arc::downgrade(stream.connection());
                // An unaccepted stream is dropped by the failed send, closing its transport.
                if let Ok(Ok(())) =
                    timeout(self.connection_timeout, self.accept_tx.send(stream)).await
                {
                    // listener.go:listenerNegotiator.finish retains accepted signal ownership.
                    while weak.upgrade().is_some_and(|c| !c.is_finished()) {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            Ok(Err(error)) => {
                debug!(conn_id, %error, "Negotiation failed");
            }
            Err(_) => {
                warn!(conn_id, "Connection timed out waiting for data channels");
                drop(pending);
                self.fail(SignalErrorCode::NegotiationTimeoutWaitingForAccept)
                    .await;
            }
        }
    }

    async fn fail(&self, code: SignalErrorCode) {
        send_terminal_error(
            self.signaling.clone(),
            self.conn_id,
            self.network_id.clone(),
            code,
        )
        .await;
    }

    /// Verifies the client identity, gathers and sends the answer.
    async fn answer(
        &self,
        pending: &Pending,
    ) -> Result<Option<crate::identity::VerifiedIdentity>, AnswerError> {
        let remote_identity = crate::identity::verify_sdp_optional(
            &self.sdp,
            crate::identity::PeerRole::Client,
            self.verify_client_token.as_deref(),
            self.allow_missing,
        )
        .await
        .map_err(|error| AnswerError::Identity(error.into()))?;

        let servers =
            crate::dialer::ice_servers(self.signaling.as_ref(), &self.ice_servers).await?;
        let nontrickle = self.signaling.disable_trickle_ice();
        if nontrickle {
            pending.connection().add_ice_servers(&servers).await?;
            pending.gathered().await?;
        }
        let mut answer = pending.describe(self.local_setup, nontrickle)?;
        if let Some(identity) = &self.server_identity {
            answer = identity.sign_sdp(&answer)?;
        }

        debug!(conn_id = self.conn_id, "Sending CONNECTRESPONSE");
        let answer_signal = Signal {
            typ: SignalType::Answer,
            connection_id: self.conn_id,
            data: answer,
            network_id: self.network_id.clone(),
        };
        if let Some(monitor) = &self.signal_monitor {
            monitor.process_outgoing(&answer_signal).await;
        }
        self.signaling.signal(answer_signal).await?;
        if !nontrickle {
            crate::dialer::trickle_candidates(
                pending.connection().clone(),
                self.signaling.clone(),
                self.network_id.clone(),
                self.conn_id,
                servers,
            );
        }
        Ok(remote_identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{idle_connection, offer_sdp};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct LocalSignaling;
    #[async_trait::async_trait]
    impl Signaling for LocalSignaling {
        async fn signal(&self, _: Signal) -> anyhow::Result<()> {
            Ok(())
        }
        fn network_id(&self) -> String {
            "local".into()
        }
    }

    fn actor(signaling: Arc<dyn Signaling>, config: NetherNetListenerConfig) -> ListenerActor {
        let (cleanup_tx, message_rx) = mpsc::channel(1);
        let (accept_tx, accept_rx) = mpsc::channel(1);
        // Keep the accept side open for the actor's lifetime in each test.
        std::mem::forget(accept_rx);
        ListenerActor {
            signaling,
            config,
            message_rx,
            accept_tx,
            connections: HashMap::new(),
            cleanup_tx,
            generated_identity_key: None,
            local: None,
        }
    }

    #[tokio::test]
    async fn stale_cleanup_does_not_remove_replacement_connection() {
        let old = idle_connection();
        let replacement = idle_connection();
        let key = ("remote".to_owned(), 42);
        let mut actor = actor(Arc::new(LocalSignaling), NetherNetListenerConfig::default());
        actor.connections.insert(
            key.clone(),
            Arc::new(ConnectionHandle {
                connection: replacement.clone(),
                candidate_budget: crate::dialer::CandidateBudget::default(),
                cancelled: tokio_util::sync::CancellationToken::new(),
            }),
        );
        actor.cleanup_connection(&key, &old);
        assert!(Arc::ptr_eq(
            &actor.connections.get(&key).unwrap().connection,
            &replacement
        ));
        actor.cleanup_connection(&key, &replacement);
        assert!(!actor.connections.contains_key(&key));
    }

    struct RecordingSignaling(mpsc::Sender<Signal>);
    #[async_trait::async_trait]
    impl Signaling for RecordingSignaling {
        async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
            self.0.send(signal).await?;
            Ok(())
        }
        fn network_id(&self) -> String {
            "local".into()
        }
        fn disable_trickle_ice(&self) -> bool {
            true
        }
    }

    fn loopback_config() -> NetherNetListenerConfig {
        NetherNetListenerConfig {
            allow_identityless_client: true,
            transport: TransportConfig::loopback_v4(),
            ..Default::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn remote_cancellation_suppresses_pending_timeout_reply() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            Arc::new(RecordingSignaling(outbound)),
            NetherNetListenerConfig {
                connection_timeout: Duration::from_secs(1),
                ..loopback_config()
            },
        );
        actor
            .handle_connect_request(Signal {
                typ: SignalType::Offer,
                connection_id: 42,
                network_id: "remote".into(),
                data: offer_sdp(),
            })
            .unwrap();
        assert_eq!(replies.recv().await.unwrap().typ, SignalType::Answer);
        actor
            .handle_signal(Signal::error(
                42,
                "remote".into(),
                SignalErrorCode::InactivityTimeout,
            ))
            .await
            .unwrap();
        tokio::time::advance(Duration::from_secs(2)).await;
        let cleanup = timeout(Duration::from_secs(1), actor.message_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let ActorMessage::CleanupConnection(key, connection) = cleanup else {
            panic!("expected worker cleanup")
        };
        actor.cleanup_connection(&key, &connection);
        assert!(!actor.connections.contains_key(&key));
        // A terminal error after cancellation would address a replacement
        // using the same tuple.
        while let Ok(reply) = replies.try_recv() {
            assert_ne!(
                reply.typ,
                SignalType::Error,
                "stale terminal reply: {reply:?}"
            );
        }
    }

    /// Stands in for an integration dependency (HTTP/JWT client behind a
    /// signaling implementation) panicking during peer-triggered negotiation.
    struct PanickingCredentials(mpsc::Sender<Signal>);
    #[async_trait::async_trait]
    impl Signaling for PanickingCredentials {
        async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
            self.0.send(signal).await?;
            Ok(())
        }
        async fn credentials(&self) -> anyhow::Result<Option<crate::Credentials>> {
            panic!("credential provider exploded")
        }
        fn network_id(&self) -> String {
            "local".into()
        }
    }

    /// Receives the worker's cleanup request and applies it.
    async fn apply_cleanup(actor: &mut ListenerActor) {
        let cleanup = timeout(Duration::from_secs(5), actor.message_rx.recv())
            .await
            .expect("worker cleanup")
            .unwrap();
        let ActorMessage::CleanupConnection(key, connection) = cleanup else {
            panic!("expected worker cleanup")
        };
        actor.cleanup_connection(&key, &connection);
    }

    async fn next_reply(replies: &mut mpsc::Receiver<Signal>) -> Signal {
        timeout(Duration::from_secs(5), replies.recv())
            .await
            .expect("reply")
            .unwrap()
    }

    fn offer(connection_id: u64) -> Signal {
        Signal {
            typ: SignalType::Offer,
            connection_id,
            network_id: "remote".into(),
            data: offer_sdp(),
        }
    }

    #[tokio::test]
    async fn negotiation_panic_fails_only_that_offer_with_typed_error() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(Arc::new(PanickingCredentials(outbound)), loopback_config());
        // The panic used to unwind through the actor task, ending the listener
        // for every later session; it must fail this offer only.
        actor.handle_signal(offer(42)).await.unwrap();
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 42);
        assert_eq!(
            reply.data,
            (SignalErrorCode::GenericFailure as i32).to_string()
        );
        apply_cleanup(&mut actor).await;
        assert!(actor.connections.is_empty());
    }

    /// A vanilla host replies `CONNECTERROR <id> 37` when it declines the
    /// offer's identity (PARITY.md S6); the listener used
    /// to stay silent, leaving the dialer to its answer timeout.
    #[tokio::test]
    async fn rejected_client_identity_replies_identity_not_allowed() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            Arc::new(RecordingSignaling(outbound)),
            NetherNetListenerConfig {
                allow_identityless_client: false,
                ..loopback_config()
            },
        );
        actor.handle_signal(offer(42)).await.unwrap();
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 42);
        assert_eq!(reply.network_id, "remote");
        assert_eq!(reply.data, "37");
        apply_cleanup(&mut actor).await;
        assert!(actor.connections.is_empty());
        assert!(replies.try_recv().is_err(), "no answer after rejection");
    }

    /// The real client's host has no negotiation deadline; a dialer whose
    /// answer arrives six seconds late (slow signaling) still connects with
    /// the default listener configuration (go-nethernet's 5 s refused it).
    #[tokio::test]
    async fn slow_answer_delivery_still_connects() {
        let (server, client) =
            crate::test_support::signaling_pair_delaying_answers(Duration::from_secs(6));
        let mut listener = NetherNetListener::bind_with_signaling(
            server,
            NetherNetListenerConfig {
                transport: TransportConfig::loopback_v4(),
                allow_identityless_client: true,
                ..Default::default()
            },
        );
        let dialer = crate::NetherNetDialer::connect_with_signaling(
            client,
            crate::NetherNetDialerConfig {
                transport: TransportConfig::loopback_v4(),
                allow_identityless_server: true,
                ..Default::default()
            },
        );
        let (dialed, accepted) = timeout(Duration::from_secs(14), async {
            tokio::join!(dialer.dial("1".into()), listener.accept())
        })
        .await
        .expect("slow negotiation timed out");
        dialed.unwrap();
        accepted.unwrap();
    }

    /// Credentials that never arrive for the first negotiation (a stalled
    /// provider) and are absent for every later one.
    struct StallingCredentials {
        replies: mpsc::Sender<Signal>,
        stalled: AtomicBool,
    }
    #[async_trait::async_trait]
    impl Signaling for StallingCredentials {
        async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
            self.replies.send(signal).await?;
            Ok(())
        }
        async fn credentials(&self) -> anyhow::Result<Option<crate::Credentials>> {
            if !self.stalled.swap(true, Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(None)
        }
        fn network_id(&self) -> String {
            "local".into()
        }
        fn disable_trickle_ice(&self) -> bool {
            true
        }
    }

    fn stalling(replies: mpsc::Sender<Signal>) -> Arc<dyn Signaling> {
        Arc::new(StallingCredentials {
            replies,
            stalled: AtomicBool::new(false),
        })
    }

    /// One offer waiting on its credentials used to hold the actor for the
    /// whole negotiation deadline, delaying every other peer's signals.
    #[tokio::test]
    async fn slow_offer_does_not_block_other_peers() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            stalling(outbound),
            NetherNetListenerConfig {
                connection_timeout: Duration::from_secs(30),
                ..loopback_config()
            },
        );
        timeout(Duration::from_secs(5), async {
            actor.handle_signal(offer(1)).await.unwrap();
            actor.handle_signal(offer(2)).await.unwrap();
        })
        .await
        .expect("a stalled offer blocked the listener actor");
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Answer);
        assert_eq!(reply.connection_id, 2);
    }

    /// An offer that cannot be answered within the negotiation deadline
    /// replies `CONNECTERROR` instead of leaving the dialer to time out.
    #[tokio::test(start_paused = true)]
    async fn slow_offer_timeout_replies_connecterror() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            stalling(outbound),
            NetherNetListenerConfig {
                connection_timeout: Duration::from_secs(1),
                ..loopback_config()
            },
        );
        actor.handle_signal(offer(7)).await.unwrap();
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 7);
        assert_eq!(
            reply.data,
            (SignalErrorCode::NegotiationTimeout as i32).to_string()
        );
        apply_cleanup(&mut actor).await;
        assert!(actor.connections.is_empty());
    }

    fn tracked() -> Arc<ConnectionHandle> {
        Arc::new(ConnectionHandle {
            connection: idle_connection(),
            candidate_budget: crate::dialer::CandidateBudget::default(),
            cancelled: tokio_util::sync::CancellationToken::new(),
        })
    }

    #[tokio::test]
    async fn negotiation_limit_rejects_with_incoming_connection_ignored() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            Arc::new(RecordingSignaling(outbound)),
            NetherNetListenerConfig {
                max_pending_connections: 1,
                ..loopback_config()
            },
        );
        actor.connections.insert(("other".into(), 1), tracked());
        actor.handle_signal(offer(2)).await.unwrap();
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 2);
        assert_eq!(
            reply.data,
            (SignalErrorCode::IncomingConnectionIgnored as i32).to_string()
        );
        assert_eq!(actor.connections.len(), 1);
    }

    #[tokio::test]
    async fn oversized_sdp_rejects_with_parsing_failure() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(
            Arc::new(RecordingSignaling(outbound)),
            NetherNetListenerConfig {
                max_sdp_size: 64,
                ..loopback_config()
            },
        );
        actor.handle_signal(offer(3)).await.unwrap();
        let reply = next_reply(&mut replies).await;
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 3);
        assert_eq!(
            reply.data,
            (SignalErrorCode::SignalingParsingFailure as i32).to_string()
        );
        assert!(actor.connections.is_empty());
    }
}
