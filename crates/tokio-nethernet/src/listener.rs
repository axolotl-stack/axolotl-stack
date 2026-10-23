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
                let duration = self.config.connection_timeout;
                let network_id = signal.network_id.clone();
                // The actor serves every session: a panic while negotiating one
                // offer must fail that offer only (its Pending closes the transport).
                let negotiation =
                    catch_dependency_panic("listener offer", self.handle_connect_request(signal));
                match timeout(duration, negotiation).await? {
                    Ok(result) => result?,
                    Err(panic) => {
                        // Bounded like listener.go:listenerNegotiator.signalError.
                        let _ = timeout(
                            Duration::from_secs(2),
                            self.signaling.signal(Signal::error(
                                conn_id,
                                network_id,
                                SignalErrorCode::GenericFailure,
                            )),
                        )
                        .await;
                        return Err(panic.into());
                    }
                }
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

    async fn handle_connect_request(&mut self, signal: Signal) -> anyhow::Result<()> {
        let conn_id = signal.connection_id;
        let network_id = signal.network_id.clone();
        self.config.stream_config.validate()?;

        // Security: Check connection limit
        if self.connections.len() >= self.config.max_pending_connections {
            warn!(
                conn_id,
                current = self.connections.len(),
                max = self.config.max_pending_connections,
                "Connection limit reached, rejecting new connection"
            );
            self.signaling
                .signal(Signal::error(
                    conn_id,
                    network_id,
                    SignalErrorCode::IncomingConnectionIgnored,
                ))
                .await?;
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
            self.signaling
                .signal(Signal::error(
                    conn_id,
                    network_id,
                    SignalErrorCode::SignalingParsingFailure,
                ))
                .await?;
            return Ok(());
        }
        let remote = sdp::parse(&signal.data)?;
        let candidate_budget = crate::dialer::CandidateBudget::default();
        candidate_budget.reserve_sdp(&signal.data)?;
        let allow_missing = self.config.allow_identityless_client
            || (!self.signaling.requires_identity() && self.config.verify_client_token.is_none());
        let verified = crate::identity::verify_sdp_optional(
            &signal.data,
            crate::identity::PeerRole::Client,
            self.config.verify_client_token.as_deref(),
            allow_missing,
        )
        .await;
        let remote_identity = match verified {
            Ok(identity) => identity,
            Err(error) => {
                // The real client's host (`InitiateIncomingSession`) and
                // go-nethernet listener.go reply 37 instead of letting the
                // dialer run into its 10-15 s answer timeout.
                warn!(conn_id, %error, "Rejecting offer: client identity not allowed");
                let _ = timeout(
                    Duration::from_secs(2),
                    self.signaling.signal(Signal::error(
                        conn_id,
                        network_id,
                        SignalErrorCode::IdentityNotAllowed,
                    )),
                )
                .await;
                return Err(error.into());
            }
        };
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

        let servers =
            crate::dialer::ice_servers(self.signaling.as_ref(), &self.config.ice_servers).await?;
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
        // A vanilla LAN host answers non-trickle (`disable_trickle_ice_for_answer`).
        let nontrickle = self.signaling.disable_trickle_ice_for_answer();
        if nontrickle {
            pending.connection().add_ice_servers(&servers).await?;
            pending.gathered().await?;
        }
        let mut answer = pending.describe(local_setup, nontrickle)?;
        if let Some(identity) = &server_identity {
            answer = identity.sign_sdp(&answer)?;
        }

        debug!(conn_id, "Sending CONNECTRESPONSE");
        let answer_signal = Signal {
            typ: SignalType::Answer,
            connection_id: conn_id,
            data: answer,
            network_id: network_id.clone(),
        };
        if let Some(monitor) = &self.config.signal_monitor {
            monitor.process_outgoing(&answer_signal).await;
        }
        self.signaling.signal(answer_signal).await?;
        if !nontrickle {
            crate::dialer::trickle_candidates(
                pending.connection().clone(),
                self.signaling.clone(),
                network_id.clone(),
                conn_id,
                servers,
            );
        }

        // A tuple remains owned until its worker/reply has finished or been cancelled.
        let cancelled = tokio_util::sync::CancellationToken::new();
        let connection = pending.connection().clone();
        self.connections.insert(
            (network_id.clone(), conn_id),
            Arc::new(ConnectionHandle {
                connection: connection.clone(),
                candidate_budget,
                cancelled: cancelled.clone(),
            }),
        );

        let accept_tx = self.accept_tx.clone();
        let connection_timeout = self.config.connection_timeout;
        let signaling = self.signaling.clone();
        let cleanup_tx = self.cleanup_tx.clone();

        tokio::spawn(async move {
            let negotiation = async {
                let opened = timeout(connection_timeout, pending.opened()).await;
                match opened {
                    Ok(Ok(())) => {
                        debug!(conn_id, "Data channels ready");
                        let stream = pending.into_stream(remote_identity);
                        let weak = Arc::downgrade(stream.connection());
                        // An unaccepted stream is dropped by the failed send, closing its transport.
                        if let Ok(Ok(())) =
                            timeout(connection_timeout, accept_tx.send(stream)).await
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
                        // dial.go:SignalErrorTimeout also bounds listener terminal notifications.
                        let _ = timeout(
                            Duration::from_secs(2),
                            signaling.signal(Signal::error(
                                conn_id,
                                network_id.clone(),
                                SignalErrorCode::NegotiationTimeoutWaitingForAccept,
                            )),
                        )
                        .await;
                    }
                }
            };
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => {}
                _ = negotiation => {}
            }

            // Release connection ownership only after failure or transport closure.
            let _ = cleanup_tx
                .send(ActorMessage::CleanupConnection(
                    (network_id, conn_id),
                    connection,
                ))
                .await;
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetherNetError;
    use crate::test_support::{idle_connection, offer_sdp};

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
            .await
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

    #[tokio::test]
    async fn negotiation_panic_fails_only_that_offer_with_typed_error() {
        let (outbound, mut replies) = mpsc::channel(4);
        let mut actor = actor(Arc::new(PanickingCredentials(outbound)), loopback_config());
        // The panic used to unwind through the actor task, ending the listener
        // for every later session; it must become this offer's error instead.
        let error = actor
            .handle_signal(Signal {
                typ: SignalType::Offer,
                connection_id: 42,
                network_id: "remote".into(),
                data: offer_sdp(),
            })
            .await
            .unwrap_err();
        match error.downcast_ref::<NetherNetError>() {
            Some(NetherNetError::DependencyPanic { context, message }) => {
                assert_eq!(*context, "listener offer");
                assert_eq!(message, "credential provider exploded");
            }
            other => panic!("expected DependencyPanic, got {other:?} ({error})"),
        }
        let reply = replies.recv().await.unwrap();
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 42);
        assert_eq!(
            reply.data,
            (SignalErrorCode::GenericFailure as i32).to_string()
        );
        assert!(actor.connections.is_empty());
    }

    /// A vanilla host replies `CONNECTERROR <id> 37` when it declines the
    /// offer's identity (client `InitiateIncomingSession`); the listener used
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
        actor
            .handle_signal(Signal {
                typ: SignalType::Offer,
                connection_id: 42,
                network_id: "remote".into(),
                data: offer_sdp(),
            })
            .await
            .unwrap_err();
        let reply = replies.recv().await.unwrap();
        assert_eq!(reply.typ, SignalType::Error);
        assert_eq!(reply.connection_id, 42);
        assert_eq!(reply.network_id, "remote");
        assert_eq!(reply.data, "37");
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
}
