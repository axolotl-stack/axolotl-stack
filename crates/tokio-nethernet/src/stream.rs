use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_util::sync::PollSender;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;

use crate::error::NetherNetError;
use crate::signaling::Signaling;

/// Maximum message size before fragmentation occurs (10000 bytes).
const MAX_MESSAGE_SIZE: usize = 10000;

/// Configuration for `NetherNetStream`.
#[derive(Debug, Clone)]
pub struct NetherNetStreamConfig {
    /// Maximum message size for fragmentation. Defaults to 10000.
    pub max_message_size: usize,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// ICE servers for WebRTC connectivity.
    pub ice_servers: Vec<RTCIceServer>,
}

impl Default for NetherNetStreamConfig {
    fn default() -> Self {
        Self {
            max_message_size: MAX_MESSAGE_SIZE,
            connection_timeout: Duration::from_secs(15),
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
        }
    }
}

/// A message to be sent or received over a NetherNet stream.
#[derive(Debug, Clone)]
pub struct Message {
    /// The payload data.
    pub buffer: Bytes,
    /// Whether the message should be sent/received via the reliable channel.
    pub reliable: bool,
}

impl Message {
    pub fn new(buffer: Bytes, reliable: bool) -> Self {
        Self { buffer, reliable }
    }

    pub fn reliable(buffer: Bytes) -> Self {
        Self {
            buffer,
            reliable: true,
        }
    }

    pub fn unreliable(buffer: Bytes) -> Self {
        Self {
            buffer,
            reliable: false,
        }
    }
}

/// A NetherNet stream wrapping WebRTC data channels.
///
/// Implements `Stream` for receiving messages and `Sink` for sending messages.
/// Handles the specific fragmentation logic used by NetherNet for reliable messages.
pub struct NetherNetStream {
    // reliable_dc: Arc<RTCDataChannel>,
    // unreliable_dc: Arc<RTCDataChannel>,
    // config: NetherNetStreamConfig,

    // Incoming messages buffer
    incoming_rx: mpsc::UnboundedReceiver<Result<Message, NetherNetError>>,
    // Internal sender kept alive to prevent receiver closure
    _incoming_tx: mpsc::UnboundedSender<Result<Message, NetherNetError>>,

    // Outbound messages sender
    outbound_tx: PollSender<Message>,

    // Fragmentation state for incoming reliable messages
    reassembly_buffer: Option<ReassemblyBuffer>,
}

struct ReassemblyBuffer {
    expected_segments: u8,
    received_segments: u8,
    data: BytesMut,
}

impl NetherNetStream {
    /// Sets up message handlers on data channels, returning the sender for incoming messages.
    ///
    /// This should be called as early as possible (e.g., inside on_data_channel callback)
    /// to avoid missing messages.
    pub fn setup_message_handlers(
        reliable_dc: &Arc<RTCDataChannel>,
        unreliable_dc: &Arc<RTCDataChannel>,
    ) -> mpsc::UnboundedSender<Result<Message, NetherNetError>> {
        let (incoming_tx, _) = mpsc::unbounded_channel();

        let tx = incoming_tx.clone();
        reliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Reliable DC on_message callback triggered"
            );
            let _ = tx.send(Ok(Message {
                buffer: data,
                reliable: true,
            }));
            Box::pin(async {})
        }));

        let tx = incoming_tx.clone();
        unreliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Unreliable DC on_message callback triggered"
            );
            let _ = tx.send(Ok(Message {
                buffer: data,
                reliable: false,
            }));
            Box::pin(async {})
        }));

        incoming_tx
    }

    /// Creates a new stream with a pre-created incoming message receiver.
    ///
    /// Use this when you already set up message handlers via `setup_message_handlers`.
    pub(crate) fn new_with_receiver(
        reliable_dc: Arc<RTCDataChannel>,
        unreliable_dc: Arc<RTCDataChannel>,
        incoming_tx: mpsc::UnboundedSender<Result<Message, NetherNetError>>,
        incoming_rx: mpsc::UnboundedReceiver<Result<Message, NetherNetError>>,
        config: NetherNetStreamConfig,
    ) -> Self {
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(100);

        let stream = Self {
            incoming_rx,
            _incoming_tx: incoming_tx,
            outbound_tx: PollSender::new(outbound_tx),
            reassembly_buffer: None,
        };

        // Spawn outbound sender task
        let r_dc = reliable_dc.clone();
        let u_dc = unreliable_dc.clone();
        let max_len = config.max_message_size;

        tokio::spawn(async move {
            while let Some(item) = outbound_rx.recv().await {
                if item.reliable {
                    let data = item.buffer;
                    let len = data.len();

                    if len <= max_len {
                        // No fragmentation needed, but we MUST prepend the segment count (0)
                        let mut buf = BytesMut::with_capacity(1 + len);
                        buf.extend_from_slice(&[0]);
                        buf.extend_from_slice(&data);
                        if let Err(e) = r_dc.send(&buf.freeze()).await {
                            tracing::error!("Failed to send reliable message: {}", e);
                        }
                    } else {
                        // Fragmentation
                        let mut segments = (len / max_len) as u8;
                        if len % max_len != 0 {
                            segments += 1;
                        }

                        let mut offset = 0;
                        while offset < len {
                            segments = segments.saturating_sub(1);

                            let end = std::cmp::min(offset + max_len, len);
                            let frag = &data[offset..end];

                            let mut buf = BytesMut::with_capacity(1 + frag.len());
                            buf.extend_from_slice(&[segments]);
                            buf.extend_from_slice(frag);

                            if let Err(e) = r_dc.send(&buf.freeze()).await {
                                tracing::error!("Failed to send reliable fragment: {}", e);
                                break;
                            }

                            offset += max_len;
                        }
                    }
                } else if let Err(e) = u_dc.send(&item.buffer).await {
                    tracing::error!("Failed to send unreliable message: {}", e);
                }
            }
        });

        stream
    }

    pub(crate) fn new(
        reliable_dc: Arc<RTCDataChannel>,
        unreliable_dc: Arc<RTCDataChannel>,
        config: NetherNetStreamConfig,
    ) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();

        // Setup callbacks for incoming messages
        let tx = incoming_tx.clone();
        reliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Reliable DC on_message callback triggered"
            );
            let _ = tx.send(Ok(Message {
                buffer: data,
                reliable: true,
            }));
            Box::pin(async {})
        }));

        let tx = incoming_tx.clone();
        unreliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Unreliable DC on_message callback triggered"
            );
            let _ = tx.send(Ok(Message {
                buffer: data,
                reliable: false,
            }));
            Box::pin(async {})
        }));

        Self::new_with_receiver(reliable_dc, unreliable_dc, incoming_tx, incoming_rx, config)
    }

    /// Connect to a NetherNet server at the given network ID using default configuration.
    ///
    /// This spawns a background task for signal processing. The caller is responsible
    /// for routing incoming signals to the returned channel.
    ///
    /// # Arguments
    /// * `network_id` - The remote network ID to connect to
    /// * `signaling` - The signaling implementation for exchanging signals
    ///
    /// # Returns
    /// A tuple of (stream, signal_sender) where signals should be routed to signal_sender.
    pub async fn connect(
        network_id: String,
        signaling: Arc<dyn Signaling>,
    ) -> Result<(Self, mpsc::Sender<crate::signaling::Signal>), NetherNetError> {
        Self::connect_with_config(network_id, signaling, NetherNetStreamConfig::default()).await
    }

    /// Connect to a NetherNet server with a custom configuration.
    ///
    /// # Arguments
    /// * `network_id` - The remote network ID to connect to
    /// * `signaling` - The signaling implementation for exchanging signals
    /// * `config` - Custom stream configuration
    ///
    /// # Returns
    /// A tuple of (stream, signal_sender) where signals should be routed to signal_sender.
    pub async fn connect_with_config(
        network_id: String,
        signaling: Arc<dyn Signaling>,
        config: NetherNetStreamConfig,
    ) -> Result<(Self, mpsc::Sender<crate::signaling::Signal>), NetherNetError> {
        use crate::dialer::{NetherNetDialer, NetherNetDialerConfig};

        let dialer_config = NetherNetDialerConfig {
            stream_config: config,
            connection_timeout: Duration::from_secs(15),
            ice_servers: vec![webrtc::ice_transport::ice_server::RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
        };

        let (dialer, signal_tx) = NetherNetDialer::new(signaling, dialer_config);
        let stream = dialer.dial(network_id).await?;
        Ok((stream, signal_tx))
    }

    /// Handles an incoming raw message from the reliable channel, performing reassembly if needed.
    fn handle_reliable_fragment(&mut self, data: Bytes) -> Option<Message> {
        if data.len() < 2 {
            return None;
        }

        let segments = data[0];
        let payload = data.slice(1..);

        if let Some(ref mut buf) = self.reassembly_buffer {
            // We are already reassembling a message.
            if buf.expected_segments != segments {
                if buf.expected_segments > 0 && buf.expected_segments - 1 != segments {
                    // Invalid sequence
                    self.reassembly_buffer = None;
                    return None;
                }
            }

            buf.expected_segments = segments;
            buf.data.extend_from_slice(&payload);
            buf.received_segments += 1;

            if segments == 0 {
                // Done!
                let full_data = std::mem::take(&mut buf.data).freeze();
                self.reassembly_buffer = None;
                return Some(Message::reliable(full_data));
            }
        } else {
            // New message start
            if segments == 0 {
                // Single packet message
                return Some(Message::reliable(payload));
            }

            // Start reassembly
            self.reassembly_buffer = Some(ReassemblyBuffer {
                expected_segments: segments,
                received_segments: 1, // First one
                data: BytesMut::from(payload.as_ref()),
            });
        }

        None
    }
}

impl Stream for NetherNetStream {
    type Item = Result<Message, NetherNetError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.incoming_rx.poll_recv(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    if msg.reliable {
                        // Handle reassembly for reliable messages
                        if let Some(full_msg) = self.handle_reliable_fragment(msg.buffer) {
                            return Poll::Ready(Some(Ok(full_msg)));
                        }
                        // Fragment incomplete, continue to get next
                        continue;
                    } else {
                        // Unreliable messages are pass-through (no fragmentation logic in Go impl for unreliable)
                        return Poll::Ready(Some(Ok(msg)));
                    }
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl Sink<Message> for NetherNetStream {
    type Error = NetherNetError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.outbound_tx
            .poll_ready_unpin(cx)
            .map_err(|_| NetherNetError::ConnectionClosed)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.outbound_tx
            .start_send_unpin(item)
            .map_err(|_| NetherNetError::ConnectionClosed)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.outbound_tx
            .poll_flush_unpin(cx)
            .map_err(|_| NetherNetError::ConnectionClosed)
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
