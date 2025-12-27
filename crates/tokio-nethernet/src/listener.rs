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
use crate::signaling::{
    Signal, SignalErrorCode, Signaling, format_ice_candidate, parse_ice_candidate, signal_type,
};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};

/// Pair of optional data channels (reliable, unreliable) protected by a mutex.
type DataChannelPair = Arc<tokio::sync::Mutex<(Option<Arc<RTCDataChannel>>, Option<Arc<RTCDataChannel>>)>>;

/// Configuration for `NetherNetListener`.
#[derive(Clone)]
pub struct NetherNetListenerConfig {
    /// Configuration for streams created by this listener.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// ICE servers for WebRTC connectivity.
    pub ice_servers: Vec<RTCIceServer>,
    /// Maximum SDP size in bytes (prevents OOM from large SDPs).
    pub max_sdp_size: usize,
    /// Maximum number of pending connections (prevents connection exhaustion).
    pub max_pending_connections: usize,
}

/// Default maximum SDP size (64 KB).
const DEFAULT_MAX_SDP_SIZE: usize = 64 * 1024;

/// Default maximum pending connections.
const DEFAULT_MAX_PENDING_CONNECTIONS: usize = 256;

impl Default for NetherNetListenerConfig {
    fn default() -> Self {
        Self {
            stream_config: NetherNetStreamConfig::default(),
            connection_timeout: Duration::from_secs(15),
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            max_sdp_size: DEFAULT_MAX_SDP_SIZE,
            max_pending_connections: DEFAULT_MAX_PENDING_CONNECTIONS,
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

struct ListenerActor {
    signaling: Arc<dyn Signaling>,
    config: NetherNetListenerConfig,
    signal_rx: mpsc::Receiver<Signal>,
    accept_tx: mpsc::Sender<NetherNetStream>,
    connections: HashMap<u64, Arc<ConnectionHandle>>,
}

struct ConnectionHandle {
    pc: Arc<RTCPeerConnection>,
    #[allow(dead_code)]
    ufrag: String,
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
        let (signal_tx, signal_rx) = mpsc::channel(128);

        let actor = ListenerActor {
            signaling,
            config,
            signal_rx,
            accept_tx,
            connections: HashMap::new(),
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
        let (signal_tx, signal_rx) = mpsc::channel(128);

        // Create actor with Arc<S> as the Signaling impl
        let signaling = std::sync::Arc::new(signaling);
        let signaling_clone = signaling.clone();

        let actor = ListenerActor {
            signaling: signaling.clone(),
            config,
            signal_rx,
            accept_tx,
            connections: HashMap::new(),
        };

        // Spawn the actor
        tokio::spawn(actor.run());

        // Spawn the signal pump - routes signals from signaling channel to actor
        tokio::spawn(async move {
            if let Some(mut rx) = signaling_clone.take_signal_receiver().await {
                while let Some(signal) = rx.recv().await {
                    if signal_tx.send(signal).await.is_err() {
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
        while let Some(signal) = self.signal_rx.recv().await {
            trace!(typ = %signal.typ, conn_id = signal.connection_id, "Processing signal");

            if let Err(e) = self.handle_signal(signal).await {
                warn!(error = %e, "Error processing signal");
            }
        }

        debug!("Listener actor shutting down");
    }

    async fn handle_signal(&mut self, signal: Signal) -> anyhow::Result<()> {
        let conn_id = signal.connection_id;

        match signal.typ.as_str() {
            signal_type::OFFER => {
                if self.connections.contains_key(&conn_id) {
                    debug!(conn_id, "Ignoring duplicate CONNECTREQUEST");
                    return Ok(());
                }
                self.handle_connect_request(signal).await?;
            }
            signal_type::CANDIDATE => {
                if let Some(handle) = self.connections.get(&conn_id) {
                    self.handle_candidate(&signal, handle).await?;
                } else {
                    debug!(conn_id, "Received CANDIDATE for unknown connection");
                }
            }
            signal_type::ERROR => {
                warn!(conn_id, data = %signal.data, "Received CONNECTERROR");
                if let Some(handle) = self.connections.remove(&conn_id) {
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
        // Parse C++ WebRTC format candidate
        let info = match parse_ice_candidate(&signal.data) {
            Ok(info) => info,
            Err(e) => {
                debug!(error = ?e, "Failed to parse ICE candidate");
                return Ok(());
            }
        };

        // Convert to RTCIceCandidateInit
        let candidate_str = format!(
            "candidate:{} 1 {} {} {} {} typ {}",
            info.foundation,
            info.protocol,
            info.priority,
            info.address,
            info.port,
            info.candidate_type
        );

        let init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
            candidate: candidate_str,
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
            username_fragment: info.ufrag,
        };

        trace!(candidate = %init.candidate, "Adding remote ICE candidate");
        handle.pc.add_ice_candidate(init).await?;

        Ok(())
    }

    async fn handle_connect_request(&mut self, signal: Signal) -> anyhow::Result<()> {
        let conn_id = signal.connection_id;
        let network_id = signal.network_id.clone();

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

        // Build WebRTC API
        let mut media_engine = MediaEngine::default();
        let registry = Registry::new();
        let registry = register_default_interceptors(registry, &mut media_engine)?;
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let rtc_config = RTCConfiguration {
            ice_servers: self.config.ice_servers.clone(),
            ..Default::default()
        };

        let pc = Arc::new(api.new_peer_connection(rtc_config).await?);

        // Setup ICE candidate handler - uses C++ WebRTC format
        let sig = self.signaling.clone();
        let net_id = network_id.clone();
        let ufrag = Arc::new(tokio::sync::Mutex::new(String::new()));
        let ufrag_clone = ufrag.clone();
        let candidate_id = Arc::new(std::sync::atomic::AtomicU32::new(0));

        pc.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
            let sig = sig.clone();
            let net_id = net_id.clone();
            let ufrag = ufrag_clone.clone();
            let id = candidate_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            Box::pin(async move {
                if let Some(candidate) = c {
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
                    let _ = sig
                        .signal(Signal {
                            typ: signal_type::CANDIDATE.to_string(),
                            connection_id: conn_id,
                            data,
                            network_id: net_id,
                        })
                        .await;
                }
            })
        }));

        // Setup data channel handler - set up message handlers IMMEDIATELY to avoid missing messages
        let (ready_tx, ready_rx) = oneshot::channel();
        let channels: DataChannelPair = Arc::new(tokio::sync::Mutex::new((None, None)));
        let channels_clone = channels.clone();
        let ready_tx = Arc::new(tokio::sync::Mutex::new(Some(ready_tx)));

        // Create message channel UPFRONT so handlers can use it immediately
        // Use bounded channel to prevent OOM from message flooding
        let channel_capacity = self.config.stream_config.incoming_channel_capacity;
        let (incoming_tx, incoming_rx) = mpsc::channel(channel_capacity);
        let incoming_tx = Arc::new(incoming_tx);
        let incoming_rx = Arc::new(tokio::sync::Mutex::new(Some(incoming_rx)));

        let incoming_tx_clone = incoming_tx.clone();
        let incoming_rx_clone = incoming_rx.clone();

        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let channels = channels_clone.clone();
            let ready_tx = ready_tx.clone();
            let incoming_tx = incoming_tx_clone.clone();
            let incoming_rx = incoming_rx_clone.clone();

            Box::pin(async move {
                let label = dc.label().to_string();
                trace!(label = %label, "Data channel opened");

                // Register message handler IMMEDIATELY before storing the channel
                // This prevents the race where data arrives before handler is set
                if label == "ReliableDataChannel" {
                    let tx = (*incoming_tx).clone();
                    dc.on_message(Box::new(move |msg| {
                        // Use try_send to avoid blocking; drop message if channel is full
                        if tx.try_send(Ok(crate::stream::Message {
                            buffer: msg.data,
                            reliable: true,
                        })).is_err() {
                            tracing::warn!("Incoming message channel full, dropping reliable message");
                        }
                        Box::pin(async {})
                    }));
                } else if label == "UnreliableDataChannel" {
                    let tx = (*incoming_tx).clone();
                    dc.on_message(Box::new(move |msg| {
                        // Use try_send to avoid blocking; drop message if channel is full
                        if tx.try_send(Ok(crate::stream::Message {
                            buffer: msg.data,
                            reliable: false,
                        })).is_err() {
                            tracing::warn!("Incoming message channel full, dropping unreliable message");
                        }
                        Box::pin(async {})
                    }));
                } else {
                    trace!(label = %label, "Ignoring unknown data channel");
                    return;
                }

                // Store channel
                let mut locked = channels.lock().await;
                if label == "ReliableDataChannel" {
                    locked.0 = Some(dc);
                } else if label == "UnreliableDataChannel" {
                    locked.1 = Some(dc);
                }

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

        pc.set_local_description(answer.clone()).await?;

        debug!(conn_id, "Sending CONNECTRESPONSE");
        self.signaling
            .signal(Signal {
                typ: signal_type::ANSWER.to_string(),
                connection_id: conn_id,
                data: answer.sdp,
                network_id: network_id.clone(),
            })
            .await?;

        // Store connection handle
        let handle = Arc::new(ConnectionHandle {
            pc: pc.clone(),
            ufrag: ufrag.lock().await.clone(),
        });
        self.connections.insert(conn_id, handle);

        // Spawn task to wait for channels and create stream
        let stream_config = self.config.stream_config.clone();
        let accept_tx = self.accept_tx.clone();
        let connection_timeout = self.config.connection_timeout;
        let pc_clone = pc.clone();
        let signaling = self.signaling.clone();

        tokio::spawn(async move {
            match timeout(connection_timeout, ready_rx).await {
                Ok(Ok((reliable, unreliable, incoming_tx, incoming_rx))) => {
                    debug!(
                        conn_id,
                        "Data channels ready, creating stream with pre-registered handlers"
                    );
                    let stream = NetherNetStream::new_with_receiver(
                        reliable,
                        unreliable,
                        incoming_tx,
                        incoming_rx,
                        stream_config,
                    );

                    if accept_tx.send(stream).await.is_err() {
                        warn!("Accept channel closed");
                    }
                }
                Ok(Err(_)) => {
                    debug!(conn_id, "Ready channel dropped");
                }
                Err(_) => {
                    warn!(conn_id, "Connection timed out waiting for data channels");
                    let _ = pc_clone.close().await;

                    // Signal timeout error
                    let _ = signaling
                        .signal(Signal::error(
                            conn_id,
                            network_id,
                            SignalErrorCode::NegotiationTimeoutWaitingForAccept,
                        ))
                        .await;
                }
            }
        });

        Ok(())
    }
}
