use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use tracing::{debug, trace, warn};
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use crate::error::NetherNetError;
use crate::signaling::{
    Signal, SignalErrorCode, Signaling, format_ice_candidate, parse_ice_candidate, signal_type,
};
use crate::stream::{NetherNetStream, NetherNetStreamConfig};

/// Configuration for `NetherNetDialer`.
#[derive(Clone)]
pub struct NetherNetDialerConfig {
    /// Configuration for streams created by this dialer.
    pub stream_config: NetherNetStreamConfig,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// ICE servers for WebRTC connectivity.
    pub ice_servers: Vec<RTCIceServer>,
}

impl Default for NetherNetDialerConfig {
    fn default() -> Self {
        Self {
            stream_config: NetherNetStreamConfig::default(),
            connection_timeout: Duration::from_secs(15),
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
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
    pending_dials: std::collections::HashMap<u64, mpsc::Sender<Signal>>,
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
        loop {
            tokio::select! {
                Some(signal) = self.signal_rx.recv() => {
                    self.dispatch_signal(signal).await;
                }
                Some(request) = self.request_rx.recv() => {
                    self.handle_request(request).await;
                }
                else => break,
            }
        }

        debug!("Dialer actor shutting down");
    }

    async fn dispatch_signal(&mut self, signal: Signal) {
        let conn_id = signal.connection_id;
        if let Some(tx) = self.pending_dials.get_mut(&conn_id) {
            if tx.send(signal).await.is_err() {
                trace!(conn_id, "Failed to dispatch signal");
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
                let connection_id = rand::random::<u64>();
                let (signal_in_tx, signal_in_rx) = mpsc::channel(32);

                self.pending_dials.insert(connection_id, signal_in_tx);

                let signaling = self.signaling.clone();
                let config = self.config.clone();

                tokio::spawn(async move {
                    let result =
                        perform_dial(connection_id, network_id, signaling, config, signal_in_rx)
                            .await;
                    let _ = response_tx.send(result);
                });
            }
        }
    }
}

/// Performs the actual dial operation.
async fn perform_dial(
    connection_id: u64,
    network_id: String,
    signaling: Arc<dyn Signaling>,
    config: NetherNetDialerConfig,
    mut signal_rx: mpsc::Receiver<Signal>,
) -> Result<NetherNetStream, NetherNetError> {
    debug!(conn_id = connection_id, network_id = %network_id, "Starting dial");

    // Build WebRTC API
    let mut media_engine = MediaEngine::default();
    let registry = Registry::new();
    let registry = register_default_interceptors(registry, &mut media_engine)
        .map_err(NetherNetError::WebRTC)?;
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    let rtc_config = RTCConfiguration {
        ice_servers: config.ice_servers.clone(),
        ..Default::default()
    };

    let pc = Arc::new(
        api.new_peer_connection(rtc_config)
            .await
            .map_err(NetherNetError::WebRTC)?,
    );

    // Setup ICE candidate handler - uses C++ WebRTC format
    let sig = signaling.clone();
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
                        connection_id,
                        data,
                        network_id: net_id,
                    })
                    .await;
            }
        })
    }));

    // Create data channels (dialer creates them, listener accepts them)
    let reliable_dc = pc
        .create_data_channel("ReliableDataChannel", None)
        .await
        .map_err(NetherNetError::WebRTC)?;

    let unreliable_init = RTCDataChannelInit {
        ordered: Some(false),
        max_retransmits: Some(0),
        ..Default::default()
    };
    let unreliable_dc = pc
        .create_data_channel("UnreliableDataChannel", Some(unreliable_init))
        .await
        .map_err(NetherNetError::WebRTC)?;

    // Create and send offer
    let offer = pc
        .create_offer(None)
        .await
        .map_err(NetherNetError::WebRTC)?;

    // Extract ufrag from SDP for candidate formatting
    if let Some(ufrag_match) = offer.sdp.split("a=ice-ufrag:").nth(1)
        && let Some(ufrag_val) = ufrag_match.split_whitespace().next()
    {
        *ufrag.lock().await = ufrag_val.to_string();
    }

    pc.set_local_description(offer.clone())
        .await
        .map_err(NetherNetError::WebRTC)?;

    debug!(conn_id = connection_id, "Sending CONNECTREQUEST");
    debug!(sdp_len = offer.sdp.len(), "SDP length");
    trace!(sdp = %offer.sdp, "Full SDP content");

    signaling
        .signal(Signal {
            typ: signal_type::OFFER.to_string(),
            connection_id,
            data: offer.sdp,
            network_id: network_id.clone(),
        })
        .await
        .map_err(NetherNetError::Signaling)?;

    // Wait for answer and candidates
    loop {
        let signal_res = timeout(config.connection_timeout, signal_rx.recv()).await;

        match signal_res {
            Ok(Some(signal)) => {
                trace!(typ = %signal.typ, "Received signal");

                match signal.typ.as_str() {
                    signal_type::ANSWER => {
                        let remote_desc = RTCSessionDescription::answer(signal.data)
                            .map_err(NetherNetError::WebRTC)?;
                        pc.set_remote_description(remote_desc)
                            .await
                            .map_err(NetherNetError::WebRTC)?;
                        debug!(conn_id = connection_id, "Answer received and set");
                        break;
                    }
                    signal_type::CANDIDATE => {
                        // Parse C++ WebRTC format candidate
                        if let Ok(info) = parse_ice_candidate(&signal.data) {
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

                            if let Err(e) = pc.add_ice_candidate(init).await {
                                warn!(error = %e, "Failed to add ICE candidate");
                            }
                        }
                    }
                    signal_type::ERROR => {
                        let code: i32 = signal.data.parse().unwrap_or(0);
                        return Err(NetherNetError::signal(
                            SignalErrorCode::None, // TODO: map code
                            format!("Remote error: {}", code),
                        ));
                    }
                    _ => {}
                }
            }
            Ok(None) => return Err(NetherNetError::ConnectionClosed),
            Err(_) => {
                let _ = signaling
                    .signal(Signal::error(
                        connection_id,
                        network_id,
                        SignalErrorCode::NegotiationTimeoutWaitingForResponse,
                    ))
                    .await;
                return Err(NetherNetError::NegotiationTimeout);
            }
        }
    }

    // Continue processing candidates until channel opens
    let (open_tx, open_rx) = oneshot::channel();
    let open_tx = Arc::new(tokio::sync::Mutex::new(Some(open_tx)));

    let open_tx_clone = open_tx.clone();
    reliable_dc.on_open(Box::new(move || {
        let open_tx = open_tx_clone.clone();
        Box::pin(async move {
            if let Some(tx) = open_tx.lock().await.take() {
                let _ = tx.send(());
            }
        })
    }));

    // Spawn candidate handler
    let pc_clone = pc.clone();
    tokio::spawn(async move {
        while let Some(signal) = signal_rx.recv().await {
            if signal.typ == signal_type::CANDIDATE
                && let Ok(info) = parse_ice_candidate(&signal.data)
            {
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

                let _ = pc_clone.add_ice_candidate(init).await;
            }
        }
    });

    // Wait for reliable channel to open
    match timeout(config.connection_timeout, open_rx).await {
        Ok(Ok(())) => {
            debug!(
                conn_id = connection_id,
                "Data channel open, connection established"
            );
            Ok(NetherNetStream::new(
                reliable_dc,
                unreliable_dc,
                config.stream_config,
            ))
        }
        Ok(Err(_)) => Err(NetherNetError::ConnectionClosed),
        Err(_) => {
            let _ = signaling
                .signal(Signal::error(
                    connection_id,
                    network_id,
                    SignalErrorCode::InactivityTimeout,
                ))
                .await;
            Err(NetherNetError::NegotiationTimeout)
        }
    }
}
