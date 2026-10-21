use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use tracing::{debug, trace, warn};
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use crate::error::NetherNetError;
use crate::signaling::{Signal, SignalErrorCode, Signaling, format_ice_candidate, signal_type};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};

/// Pair of optional data channels (reliable, unreliable) protected by a mutex.
type DataChannelPair =
    Arc<tokio::sync::Mutex<(Option<Arc<RTCDataChannel>>, Option<Arc<RTCDataChannel>>)>>;

/// Configuration for `NetherNetListener`.
#[derive(Clone)]
pub struct NetherNetListenerConfig {
    /// Configuration for streams created by this listener.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// Explicit ICE servers; nonempty values override signaling credentials.
    pub ice_servers: Vec<RTCIceServer>,
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
            // df-mc/go-nethernet listener.go:ListenConfig.connContext uses five seconds.
            connection_timeout: Duration::from_secs(5),
            // credentials.go:gatherOptions uses host candidates when credentials are absent.
            ice_servers: Vec::new(),
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
    CleanupConnection((String, u64), Arc<RTCPeerConnection>),
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
}

struct ConnectionHandle {
    pc: Arc<RTCPeerConnection>,
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
    pub async fn accept(&mut self) -> Result<NetherNetStream, NetherNetError> {
        self.accept_rx
            .recv()
            .await
            .ok_or(NetherNetError::ConnectionClosed)
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
                ActorMessage::CleanupConnection(key, pc) => self.cleanup_connection(&key, &pc),
            }
        }

        for handle in self.connections.into_values() {
            handle.cancelled.cancel();
            if let Err(error) = handle.pc.close().await {
                debug!(%error, "Closing listener peer failed");
            }
        }
        debug!("Listener actor shutting down");
    }

    fn cleanup_connection(&mut self, key: &(String, u64), pc: &Arc<RTCPeerConnection>) {
        // listener.go:listenerNegotiator.unregister must not remove a replacement owner.
        if self
            .connections
            .get(key)
            .is_some_and(|handle| Arc::ptr_eq(&handle.pc, pc))
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

        match signal.typ.as_str() {
            signal_type::OFFER => {
                if self.connections.contains_key(&key) {
                    debug!(conn_id, "Ignoring duplicate CONNECTREQUEST");
                    return Ok(());
                }
                let duration = self.config.connection_timeout;
                timeout(duration, self.handle_connect_request(signal)).await??;
            }
            signal_type::CANDIDATE => {
                if let Some(handle) = self.connections.get(&key) {
                    self.handle_candidate(&signal, handle).await?;
                } else {
                    debug!(conn_id, "Received CANDIDATE for unknown connection");
                }
            }
            signal_type::ERROR => {
                warn!(conn_id, data = %signal.data, from = %signal.network_id, "Received CONNECTERROR");
                if let Some(handle) = self.connections.get(&key) {
                    // Keep tuple ownership until its cancelled worker has stopped replying.
                    handle.cancelled.cancel();
                    let _ = handle.pc.close().await;
                }
            }
            _ => {
                debug!(typ = %signal.typ, "Unknown signal type");
            }
        }

        Ok(())
    }

    async fn handle_candidate(
        &self,
        signal: &Signal,
        handle: &ConnectionHandle,
    ) -> anyhow::Result<()> {
        crate::dialer::add_candidate(&handle.pc, &signal.data, &handle.candidate_budget).await?;
        Ok(())
    }

    async fn handle_connect_request(&mut self, signal: Signal) -> anyhow::Result<()> {
        let conn_id = signal.connection_id;
        let network_id = signal.network_id.clone();
        self.config.stream_config.validate()?;
        let mut stream_config = self.config.stream_config.clone();
        stream_config.max_message_size = crate::stream::negotiate_max_message_size(
            &signal.data,
            stream_config.max_message_size,
        )?;

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
        let candidate_budget = crate::dialer::CandidateBudget::default();
        candidate_budget.reserve_sdp(&signal.data)?;
        let allow_missing = self.config.allow_identityless_client
            || (!self.signaling.requires_identity() && self.config.verify_client_token.is_none());
        let remote_identity = crate::identity::verify_sdp_optional(
            &signal.data,
            crate::identity::PeerRole::Client,
            self.config.verify_client_token.as_deref(),
            allow_missing,
        )
        .await?;
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

        // Build WebRTC API
        let mut media_engine = MediaEngine::default();
        let registry = Registry::new();
        let registry = register_default_interceptors(registry, &mut media_engine)?;
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let rtc_config = RTCConfiguration {
            ice_servers: crate::dialer::ice_servers(
                self.signaling.as_ref(),
                &self.config.ice_servers,
            )
            .await?,
            ..Default::default()
        };

        let pc = Arc::new(api.new_peer_connection(rtc_config).await?);
        let mut guard = crate::dialer::PeerGuard(Some(pc.clone()));

        // Setup ICE candidate handler - uses C++ WebRTC format
        let sig = self.signaling.clone();
        let net_id = network_id.clone();
        let ufrag = Arc::new(tokio::sync::Mutex::new(String::new()));
        let ufrag_clone = ufrag.clone();
        let candidate_id = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let nontrickle = self.signaling.disable_trickle_ice();
        let (started_tx, started_rx) = tokio::sync::watch::channel(false);
        if !nontrickle {
            pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
                let sig = sig.clone();
                let net_id = net_id.clone();
                let ufrag = ufrag_clone.clone();
                let id = candidate_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut started = started_rx.clone();

                Box::pin(async move {
                    if let Some(candidate) = c {
                        if started.wait_for(|ready| *ready).await.is_err() {
                            return;
                        }
                        let ufrag_val = ufrag.lock().await;
                        let data = format_ice_candidate(
                            id,
                            &candidate.foundation,
                            &candidate.protocol.to_string().to_lowercase(),
                            candidate.priority,
                            &candidate.address,
                            candidate.port,
                            &candidate.typ.to_string().to_lowercase(),
                            if candidate.related_address.is_empty() {
                                None
                            } else {
                                Some(&candidate.related_address)
                            },
                            if candidate.related_port == 0 {
                                None
                            } else {
                                Some(candidate.related_port)
                            },
                            &ufrag_val,
                        );

                        trace!(candidate = %data, "Sending ICE candidate");
                        if !matches!(
                            timeout(
                                Duration::from_secs(15),
                                sig.signal(Signal {
                                    typ: signal_type::CANDIDATE.to_string(),
                                    connection_id: conn_id,
                                    data,
                                    network_id: net_id,
                                })
                            )
                            .await,
                            Ok(Ok(()))
                        ) {
                            warn!(conn_id, "Failed to signal ICE candidate");
                        }
                    }
                })
            }));
        }

        // Setup data channel handler - set up message handlers IMMEDIATELY to avoid missing messages
        let (ready_tx, ready_rx) = oneshot::channel();
        let channels: DataChannelPair = Arc::new(tokio::sync::Mutex::new((None, None)));
        let channels_clone = channels.clone();
        let ready_tx = Arc::new(tokio::sync::Mutex::new(Some(ready_tx)));

        // Create message channel UPFRONT so handlers can use it immediately
        // Use bounded channel to prevent OOM from message flooding
        let channel_capacity = stream_config.incoming_channel_capacity;
        let (incoming_tx, incoming_rx) = mpsc::channel(channel_capacity);
        let incoming_tx = Arc::new(incoming_tx);
        let incoming_rx = Arc::new(tokio::sync::Mutex::new(Some(incoming_rx)));

        let incoming_tx_clone = incoming_tx.clone();
        let incoming_rx_clone = incoming_rx.clone();
        let weak_pc = Arc::downgrade(&pc);

        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let channels = channels_clone.clone();
            let ready_tx = ready_tx.clone();
            let incoming_tx = incoming_tx_clone.clone();
            let incoming_rx = incoming_rx_clone.clone();
            let weak_pc = weak_pc.clone();

            Box::pin(async move {
                let label = dc.label().to_string();
                trace!(label = %label, "Data channel opened");

                // message.go:MessageReliability.Check rejects unexpected channels or parameters.
                let reliable = match label.as_str() {
                    "ReliableDataChannel" if dc.ordered() && dc.max_retransmits().is_none() => true,
                    "UnreliableDataChannel" if !dc.ordered() && dc.max_retransmits() == Some(0) => {
                        false
                    }
                    _ => {
                        if let Some(pc) = weak_pc.upgrade() {
                            let _ = pc.close().await;
                        }
                        return;
                    }
                };
                if !dc.protocol().is_empty()
                    || dc.negotiated()
                    || dc.max_packet_lifetime().is_some()
                {
                    if let Some(pc) = weak_pc.upgrade() {
                        let _ = pc.close().await;
                    }
                    return;
                }
                crate::stream::install_message_handler(&dc, (*incoming_tx).clone(), reliable);

                // Store channel
                let mut locked = channels.lock().await;
                let slot = if reliable {
                    &mut locked.0
                } else {
                    &mut locked.1
                };
                if slot.is_some() || ready_tx.lock().await.is_none() {
                    drop(locked);
                    if let Some(pc) = weak_pc.upgrade() {
                        tokio::spawn(async move {
                            let _ = pc.close().await;
                        });
                    }
                    return;
                }
                *slot = Some(dc);

                // When both channels are ready, signal completion
                if locked.0.is_some() && locked.1.is_some() {
                    trace!("Both data channels ready");

                    if let Some(tx) = ready_tx.lock().await.take() {
                        let r = locked.0.take().unwrap();
                        let u = locked.1.take().unwrap();
                        let rx = incoming_rx.lock().await.take();
                        if let Some(rx) = rx {
                            let _ = tx.send((r, u, (*incoming_tx).clone(), rx));
                        }
                    }
                }
            })
        }));

        // Set remote description (offer)
        debug!(
            conn_id,
            sdp_len = signal.data.len(),
            "Setting remote description"
        );
        trace!(conn_id, sdp = %signal.data, "Full SDP content");

        // Validate SDP has ice-ufrag before trying to set
        if !signal.data.contains("a=ice-ufrag:") {
            anyhow::bail!("SDP missing ice-ufrag - possibly truncated in signaling");
        }

        let remote_desc = RTCSessionDescription::offer(signal.data.clone())?;
        pc.set_remote_description(remote_desc).await?;

        // Create and send answer
        let answer = pc.create_answer(None).await?;

        // Extract ufrag from SDP for candidate formatting
        if let Some(ufrag_match) = answer.sdp.split("a=ice-ufrag:").nth(1)
            && let Some(ufrag_val) = ufrag_match.split_whitespace().next()
        {
            *ufrag.lock().await = ufrag_val.to_string();
        }

        let mut fixed_sdp = crate::stream::local_sdp(answer.sdp.clone());
        if let Some(identity) = &server_identity {
            fixed_sdp = identity.sign_sdp(&fixed_sdp)?;
        }
        let mut gathering = pc.gathering_complete_promise().await;
        // webrtc-rs requires its original local answer; wire-only attributes are added above.
        pc.set_local_description(answer).await?;
        if nontrickle {
            gathering.recv().await;
            fixed_sdp = crate::stream::local_sdp(
                pc.local_description()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("Peer closed during ICE gathering"))?
                    .sdp,
            );
            if let Some(identity) = &server_identity {
                fixed_sdp = identity.sign_sdp(&fixed_sdp)?;
            }
        }

        debug!(conn_id, "Sending CONNECTRESPONSE");
        let answer_signal = Signal {
            typ: signal_type::ANSWER.to_string(),
            connection_id: conn_id,
            data: fixed_sdp,
            network_id: network_id.clone(),
        };
        if let Some(monitor) = &self.config.signal_monitor {
            monitor.process_outgoing(&answer_signal).await;
        }
        self.signaling.signal(answer_signal).await?;
        let _ = started_tx.send(true);

        // A tuple remains owned until its worker/reply has finished or been cancelled.
        let cancelled = tokio_util::sync::CancellationToken::new();
        let handle = Arc::new(ConnectionHandle {
            pc: pc.clone(),
            candidate_budget,
            cancelled: cancelled.clone(),
        });
        self.connections
            .insert((network_id.clone(), conn_id), handle);
        guard.0 = None;

        // Spawn task to wait for channels and create stream
        let accept_tx = self.accept_tx.clone();
        let connection_timeout = self.config.connection_timeout;
        let pc_clone = pc.clone();
        let signaling = self.signaling.clone();
        let cleanup_tx = self.cleanup_tx.clone();

        tokio::spawn(async move {
            let negotiation = async {
                match timeout(connection_timeout, ready_rx).await {
                    Ok(Ok((reliable, unreliable, incoming_tx, incoming_rx))) => {
                        debug!(
                            conn_id,
                            "Data channels ready, creating stream with pre-registered handlers"
                        );
                        let mut stream = NetherNetStream::new_with_receiver(
                            reliable,
                            unreliable,
                            incoming_tx,
                            incoming_rx,
                            stream_config,
                        );
                        stream.attach_peer(pc_clone.clone());
                        stream.set_remote_identity(remote_identity);

                        match timeout(connection_timeout, accept_tx.send(stream)).await {
                            Ok(Ok(())) => {
                                // listener.go:listenerNegotiator.finish retains accepted signal ownership.
                                while !matches!(pc_clone.connection_state(),
                                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Closed |
                                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Failed)
                            {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                            }
                            _ => {
                                let _ = pc_clone.close().await;
                            }
                        }
                    }
                    Ok(Err(_)) => {
                        debug!(conn_id, "Ready channel dropped");
                        let _ = pc_clone.close().await;
                    }
                    Err(_) => {
                        warn!(conn_id, "Connection timed out waiting for data channels");
                        let _ = pc_clone.close().await;

                        // Signal timeout error
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
                    pc_clone,
                ))
                .await;
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn stale_cleanup_does_not_remove_replacement_connection() {
        let api = APIBuilder::new().build();
        let old = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let replacement = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        let key = ("remote".to_owned(), 42);
        let (cleanup_tx, message_rx) = mpsc::channel(1);
        let (accept_tx, _accept_rx) = mpsc::channel(1);
        let mut actor = ListenerActor {
            signaling: Arc::new(LocalSignaling),
            config: NetherNetListenerConfig::default(),
            message_rx,
            accept_tx,
            connections: HashMap::from([(
                key.clone(),
                Arc::new(ConnectionHandle {
                    pc: replacement.clone(),
                    candidate_budget: crate::dialer::CandidateBudget::default(),
                    cancelled: tokio_util::sync::CancellationToken::new(),
                }),
            )]),
            cleanup_tx,
            generated_identity_key: None,
        };
        actor.cleanup_connection(&key, &old);
        assert!(Arc::ptr_eq(
            &actor.connections.get(&key).unwrap().pc,
            &replacement
        ));
        actor.cleanup_connection(&key, &replacement);
        assert!(!actor.connections.contains_key(&key));
        old.close().await.unwrap();
        replacement.close().await.unwrap();
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
    }

    #[tokio::test(start_paused = true)]
    async fn remote_cancellation_suppresses_pending_timeout_reply() {
        let api = APIBuilder::new().build();
        let client = api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        client
            .create_data_channel("ReliableDataChannel", None)
            .await
            .unwrap();
        let offer = client.create_offer(None).await.unwrap();
        let (outbound, mut replies) = mpsc::channel(4);
        let (cleanup_tx, message_rx) = mpsc::channel(1);
        let (accept_tx, _accept_rx) = mpsc::channel(1);
        let mut actor = ListenerActor {
            signaling: Arc::new(RecordingSignaling(outbound)),
            config: NetherNetListenerConfig {
                allow_identityless_client: true,
                connection_timeout: Duration::from_secs(1),
                ..Default::default()
            },
            message_rx,
            accept_tx,
            connections: HashMap::new(),
            cleanup_tx,
            generated_identity_key: None,
        };
        actor
            .handle_connect_request(Signal {
                typ: signal_type::OFFER.into(),
                connection_id: 42,
                network_id: "remote".into(),
                data: crate::stream::local_sdp(offer.sdp),
            })
            .await
            .unwrap();
        assert_eq!(replies.recv().await.unwrap().typ, signal_type::ANSWER);
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
        let ActorMessage::CleanupConnection(key, pc) = cleanup else {
            panic!("expected worker cleanup")
        };
        actor.cleanup_connection(&key, &pc);
        assert!(!actor.connections.contains_key(&key));
        // Previously queued trickle candidates are legitimate; a terminal error
        // after cancellation would address a replacement using the same tuple.
        while let Ok(reply) = replies.try_recv() {
            assert_ne!(
                reply.typ,
                signal_type::ERROR,
                "stale terminal reply: {reply:?}"
            );
        }
        client.close().await.unwrap();
    }
}
