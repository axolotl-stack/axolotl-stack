use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
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

/// Default maximum reassembly buffer size (10 MB).
const DEFAULT_MAX_REASSEMBLY_SIZE: usize = 10 * 1024 * 1024;

/// Default reassembly timeout (30 seconds).
const DEFAULT_REASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for `NetherNetStream`.
#[derive(Debug, Clone)]
pub struct NetherNetStreamConfig {
    /// Maximum message size for fragmentation. Defaults to 10000.
    pub max_message_size: usize,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// ICE servers for WebRTC connectivity.
    pub ice_servers: Vec<RTCIceServer>,
    /// Maximum size of the reassembly buffer in bytes.
    /// Prevents OOM attacks from fragmented messages.
    pub max_reassembly_size: usize,
    /// Timeout for reassembling fragmented messages.
    /// Drops incomplete fragments after this duration.
    pub reassembly_timeout: Duration,
    /// Capacity of the incoming message channel.
    pub incoming_channel_capacity: usize,
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
            max_reassembly_size: DEFAULT_MAX_REASSEMBLY_SIZE,
            reassembly_timeout: DEFAULT_REASSEMBLY_TIMEOUT,
            incoming_channel_capacity: 1024,
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
    // Incoming messages buffer
    incoming_rx: mpsc::Receiver<Result<Message, NetherNetError>>,
    // Internal sender kept alive to prevent receiver closure
    _incoming_tx: mpsc::Sender<Result<Message, NetherNetError>>,

    // Outbound messages sender
    outbound_tx: PollSender<Message>,

    // Fragmentation state for incoming reliable messages
    reassembly_buffer: Option<ReassemblyBuffer>,

    // Config for limits
    max_reassembly_size: usize,
    reassembly_timeout: Duration,
}

struct ReassemblyBuffer {
    expected_segments: u8,
    received_segments: u8,
    data: BytesMut,
    total_size: usize,
    started_at: Instant,
}

impl NetherNetStream {
    /// Sets up message handlers on data channels, returning the sender for incoming messages.
    ///
    /// This should be called as early as possible (e.g., inside on_data_channel callback)
    /// to avoid missing messages.
    ///
    /// # Arguments
    /// * `reliable_dc` - The reliable data channel
    /// * `unreliable_dc` - The unreliable data channel
    /// * `capacity` - Channel capacity (defaults to 1024 if 0)
    pub fn setup_message_handlers(
        reliable_dc: &Arc<RTCDataChannel>,
        unreliable_dc: &Arc<RTCDataChannel>,
        capacity: usize,
    ) -> mpsc::Sender<Result<Message, NetherNetError>> {
        let capacity = if capacity == 0 { 1024 } else { capacity };
        let (incoming_tx, _) = mpsc::channel(capacity);

        let tx = incoming_tx.clone();
        reliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Reliable DC on_message callback triggered"
            );
            // Use try_send to avoid blocking; drop message if channel is full
            if tx
                .try_send(Ok(Message {
                    buffer: data,
                    reliable: true,
                }))
                .is_err()
            {
                tracing::warn!("Incoming message channel full, dropping reliable message");
            }
            Box::pin(async {})
        }));

        let tx = incoming_tx.clone();
        unreliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Unreliable DC on_message callback triggered"
            );
            // Use try_send to avoid blocking; drop message if channel is full
            if tx
                .try_send(Ok(Message {
                    buffer: data,
                    reliable: false,
                }))
                .is_err()
            {
                tracing::warn!("Incoming message channel full, dropping unreliable message");
            }
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
        incoming_tx: mpsc::Sender<Result<Message, NetherNetError>>,
        incoming_rx: mpsc::Receiver<Result<Message, NetherNetError>>,
        config: NetherNetStreamConfig,
    ) -> Self {
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(100);

        let stream = Self {
            incoming_rx,
            _incoming_tx: incoming_tx,
            outbound_tx: PollSender::new(outbound_tx),
            reassembly_buffer: None,
            max_reassembly_size: config.max_reassembly_size,
            reassembly_timeout: config.reassembly_timeout,
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
        let capacity = config.incoming_channel_capacity;
        let (incoming_tx, incoming_rx) = mpsc::channel(capacity);

        // Setup callbacks for incoming messages
        let tx = incoming_tx.clone();
        reliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Reliable DC on_message callback triggered"
            );
            // Use try_send to avoid blocking; drop message if channel is full
            if tx
                .try_send(Ok(Message {
                    buffer: data,
                    reliable: true,
                }))
                .is_err()
            {
                tracing::warn!("Incoming message channel full, dropping reliable message");
            }
            Box::pin(async {})
        }));

        let tx = incoming_tx.clone();
        unreliable_dc.on_message(Box::new(move |msg| {
            let data = msg.data;
            tracing::debug!(
                len = data.len(),
                "Unreliable DC on_message callback triggered"
            );
            // Use try_send to avoid blocking; drop message if channel is full
            if tx
                .try_send(Ok(Message {
                    buffer: data,
                    reliable: false,
                }))
                .is_err()
            {
                tracing::warn!("Incoming message channel full, dropping unreliable message");
            }
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
    /// Enforces max_reassembly_size and reassembly_timeout limits.
    fn handle_reliable_fragment(&mut self, data: Bytes) -> Option<Message> {
        if data.len() < 2 {
            return None;
        }

        let segments = data[0];
        let payload = data.slice(1..);
        let now = Instant::now();

        if let Some(ref mut buf) = self.reassembly_buffer {
            // Check timeout - drop incomplete reassembly if too old
            if now.duration_since(buf.started_at) > self.reassembly_timeout {
                tracing::warn!(
                    elapsed_secs = now.duration_since(buf.started_at).as_secs(),
                    "Reassembly timeout exceeded, dropping incomplete message"
                );
                self.reassembly_buffer = None;
                return None;
            }

            // Check size limit - reject if would exceed max
            if buf.total_size + payload.len() > self.max_reassembly_size {
                tracing::warn!(
                    current_size = buf.total_size,
                    payload_len = payload.len(),
                    max_size = self.max_reassembly_size,
                    "Reassembly buffer size limit exceeded, dropping message"
                );
                self.reassembly_buffer = None;
                return None;
            }

            // We are already reassembling a message.
            if buf.expected_segments != segments
                && buf.expected_segments > 0
                && buf.expected_segments - 1 != segments
            {
                // Invalid sequence
                self.reassembly_buffer = None;
                return None;
            }

            buf.expected_segments = segments;
            buf.data.extend_from_slice(&payload);
            buf.total_size += payload.len();
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

            // Check if initial fragment exceeds limit
            if payload.len() > self.max_reassembly_size {
                tracing::warn!(
                    payload_len = payload.len(),
                    max_size = self.max_reassembly_size,
                    "Initial fragment exceeds max reassembly size"
                );
                return None;
            }

            // Start reassembly
            self.reassembly_buffer = Some(ReassemblyBuffer {
                expected_segments: segments,
                received_segments: 1, // First one
                data: BytesMut::from(payload.as_ref()),
                total_size: payload.len(),
                started_at: now,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper that creates a minimal reassembly test harness
    struct ReassemblyTestHarness {
        reassembly_buffer: Option<ReassemblyBuffer>,
        max_reassembly_size: usize,
        reassembly_timeout: Duration,
    }

    impl ReassemblyTestHarness {
        fn new(max_size: usize, timeout: Duration) -> Self {
            Self {
                reassembly_buffer: None,
                max_reassembly_size: max_size,
                reassembly_timeout: timeout,
            }
        }

        fn handle_fragment(&mut self, data: Bytes) -> Option<Message> {
            if data.len() < 2 {
                return None;
            }

            let segments = data[0];
            let payload = data.slice(1..);
            let now = Instant::now();

            if let Some(ref mut buf) = self.reassembly_buffer {
                // Check timeout
                if now.duration_since(buf.started_at) > self.reassembly_timeout {
                    self.reassembly_buffer = None;
                    return None;
                }

                // Check size limit
                if buf.total_size + payload.len() > self.max_reassembly_size {
                    self.reassembly_buffer = None;
                    return None;
                }

                if buf.expected_segments != segments {
                    if buf.expected_segments > 0 && buf.expected_segments - 1 != segments {
                        self.reassembly_buffer = None;
                        return None;
                    }
                }

                buf.expected_segments = segments;
                buf.data.extend_from_slice(&payload);
                buf.total_size += payload.len();
                buf.received_segments += 1;

                if segments == 0 {
                    let full_data = std::mem::take(&mut buf.data).freeze();
                    self.reassembly_buffer = None;
                    return Some(Message::reliable(full_data));
                }
            } else {
                if segments == 0 {
                    return Some(Message::reliable(payload));
                }

                if payload.len() > self.max_reassembly_size {
                    return None;
                }

                self.reassembly_buffer = Some(ReassemblyBuffer {
                    expected_segments: segments,
                    received_segments: 1,
                    data: BytesMut::from(payload.as_ref()),
                    total_size: payload.len(),
                    started_at: now,
                });
            }

            None
        }
    }

    #[test]
    fn test_single_packet_no_reassembly() {
        let mut harness = ReassemblyTestHarness::new(1024 * 1024, Duration::from_secs(30));

        // segments=0 means single packet
        let data = Bytes::from(vec![0u8, b'h', b'e', b'l', b'l', b'o']);
        let result = harness.handle_fragment(data);

        assert!(result.is_some());
        let msg = result.unwrap();
        assert!(msg.reliable);
        assert_eq!(msg.buffer.as_ref(), b"hello");
    }

    #[test]
    fn test_fragmented_message_reassembly() {
        let mut harness = ReassemblyTestHarness::new(1024 * 1024, Duration::from_secs(30));

        // First fragment: segments=2 (2 more to follow)
        let frag1 = Bytes::from(vec![2u8, b'h', b'e', b'l']);
        let result1 = harness.handle_fragment(frag1);
        assert!(result1.is_none()); // Not complete yet

        // Second fragment: segments=1 (1 more to follow)
        let frag2 = Bytes::from(vec![1u8, b'l', b'o', b' ']);
        let result2 = harness.handle_fragment(frag2);
        assert!(result2.is_none()); // Not complete yet

        // Third fragment: segments=0 (this is the last one)
        let frag3 = Bytes::from(vec![0u8, b'w', b'o', b'r', b'l', b'd']);
        let result3 = harness.handle_fragment(frag3);
        assert!(result3.is_some());

        let msg = result3.unwrap();
        assert_eq!(msg.buffer.as_ref(), b"hello world");
    }

    #[test]
    fn test_reassembly_size_limit_exceeded() {
        // Only allow 10 bytes total
        let mut harness = ReassemblyTestHarness::new(10, Duration::from_secs(30));

        // First fragment: 5 bytes payload
        let frag1 = Bytes::from(vec![1u8, b'h', b'e', b'l', b'l', b'o']);
        let result1 = harness.handle_fragment(frag1);
        assert!(result1.is_none()); // Not complete yet, buffer size is 5

        // Second fragment: 6 bytes payload would exceed 10 byte limit
        let frag2 = Bytes::from(vec![0u8, b' ', b'w', b'o', b'r', b'l', b'd']);
        let result2 = harness.handle_fragment(frag2);
        assert!(result2.is_none()); // Should be rejected due to size limit

        // Buffer should be cleared
        assert!(harness.reassembly_buffer.is_none());
    }

    #[test]
    fn test_initial_fragment_exceeds_size_limit() {
        // Only allow 5 bytes total
        let mut harness = ReassemblyTestHarness::new(5, Duration::from_secs(30));

        // First fragment: 10 bytes payload exceeds limit
        let frag = Bytes::from(vec![
            1u8, b'h', b'e', b'l', b'l', b'o', b' ', b'w', b'o', b'r', b'l',
        ]);
        let result = harness.handle_fragment(frag);
        assert!(result.is_none());

        // Buffer should not be created
        assert!(harness.reassembly_buffer.is_none());
    }

    #[test]
    fn test_invalid_segment_sequence() {
        let mut harness = ReassemblyTestHarness::new(1024 * 1024, Duration::from_secs(30));

        // First fragment: segments=2
        let frag1 = Bytes::from(vec![2u8, b'h', b'e', b'l']);
        harness.handle_fragment(frag1);

        // Invalid: should be 1, but got 5
        let frag2 = Bytes::from(vec![5u8, b'l', b'o']);
        let result = harness.handle_fragment(frag2);
        assert!(result.is_none());

        // Buffer should be cleared due to invalid sequence
        assert!(harness.reassembly_buffer.is_none());
    }

    #[test]
    fn test_empty_fragment_rejected() {
        let mut harness = ReassemblyTestHarness::new(1024 * 1024, Duration::from_secs(30));

        // Fragment too short (< 2 bytes)
        let data = Bytes::from(vec![0u8]);
        let result = harness.handle_fragment(data);
        assert!(result.is_none());
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::reliable(Bytes::from("test"));
        assert!(msg.reliable);
        assert_eq!(msg.buffer.as_ref(), b"test");

        let msg = Message::unreliable(Bytes::from("test2"));
        assert!(!msg.reliable);
        assert_eq!(msg.buffer.as_ref(), b"test2");

        let msg = Message::new(Bytes::from("test3"), true);
        assert!(msg.reliable);
    }

    #[test]
    fn test_default_stream_config() {
        let config = NetherNetStreamConfig::default();

        assert_eq!(config.max_message_size, 10000);
        assert_eq!(config.connection_timeout, Duration::from_secs(15));
        assert_eq!(config.max_reassembly_size, 10 * 1024 * 1024);
        assert_eq!(config.reassembly_timeout, Duration::from_secs(30));
        assert_eq!(config.incoming_channel_capacity, 1024);
        assert!(!config.ice_servers.is_empty());
    }
}
