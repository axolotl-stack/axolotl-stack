use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::time::Duration;
use tokio_util::sync::PollSender;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;

use crate::error::NetherNetError;
use crate::signaling::SignalingChannel;

/// Payload ceiling from upstream `conn.go:maxMessageSize` and `description.encode`:
/// 262144 SCTP bytes minus the one-byte remaining-segment counter.
pub(crate) const MAX_MESSAGE_SIZE: usize = 262143;

/// Forced webrtc-rs 0.12 deviation from `conn.go:maxMessageSize`: its
/// `data_channel/mod.rs:RTCDataChannel::read_loop` uses a fixed 65535-byte
/// DATA_CHANNEL_BUFFER_SIZE. Reserve one byte for the NetherNet header.
pub(crate) const TRANSPORT_MAX_PAYLOAD: usize = 65534;

/// Local bounded-state policy, not a wire constant.
const DEFAULT_MAX_REASSEMBLY_SIZE: usize = MAX_MESSAGE_SIZE * 255;

/// Default reassembly timeout (30 seconds).
const DEFAULT_REASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for `NetherNetStream`.
#[derive(Debug, Clone)]
pub struct NetherNetStreamConfig {
    /// Outbound segment payload size, at most 65534; does not limit inbound frames.
    pub max_message_size: usize,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// ICE servers for WebRTC connectivity.
    pub ice_servers: Vec<RTCIceServer>,
    /// Maximum size of the reassembly buffer in bytes.
    /// Prevents OOM attacks from fragmented messages.
    pub max_reassembly_size: usize,
    /// Timeout for reassembling fragmented messages.
    /// Rejects subsequent fragments once this duration has elapsed.
    pub reassembly_timeout: Duration,
    /// Capacity of the incoming message channel.
    pub incoming_channel_capacity: usize,
}

impl Default for NetherNetStreamConfig {
    fn default() -> Self {
        Self {
            max_message_size: TRANSPORT_MAX_PAYLOAD,
            // dial.go:Dialer.Dial and credentials.go:gatherOptions.
            connection_timeout: Duration::from_secs(15),
            ice_servers: Vec::new(),
            max_reassembly_size: DEFAULT_MAX_REASSEMBLY_SIZE,
            reassembly_timeout: DEFAULT_REASSEMBLY_TIMEOUT,
            // message.go:wrapDataChannel uses an unbuffered packet handoff;
            // one slot is the smallest Tokio bounded asynchronous handoff.
            incoming_channel_capacity: 1,
        }
    }
}

impl NetherNetStreamConfig {
    pub(crate) fn validate(&self) -> Result<(), NetherNetError> {
        if self.max_message_size == 0 || self.max_message_size > TRANSPORT_MAX_PAYLOAD {
            return Err(NetherNetError::InvalidConfiguration(
                "segment size must be 1..=65534",
            ));
        }
        if self.incoming_channel_capacity == 0 || self.incoming_channel_capacity > 1024 {
            return Err(NetherNetError::InvalidConfiguration(
                "incoming capacity must be 1..=1024",
            ));
        }
        if self.max_reassembly_size == 0 || self.max_reassembly_size > DEFAULT_MAX_REASSEMBLY_SIZE {
            return Err(NetherNetError::InvalidConfiguration(
                "reassembly size must be 1..=66846465",
            ));
        }
        Ok(())
    }

    fn into_dialer_config(self) -> crate::dialer::NetherNetDialerConfig {
        crate::dialer::NetherNetDialerConfig {
            connection_timeout: self.connection_timeout,
            ice_servers: self.ice_servers.clone(),
            stream_config: self,
            ..Default::default()
        }
    }
}

/// Negotiated payload limit, following upstream `conn.go:parseDescription`
/// and `dial.go:Dialer.startTransports` (SCTP limit minus one header byte).
pub(crate) fn negotiate_max_message_size(sdp: &str, local: usize) -> Result<usize, NetherNetError> {
    let value = sdp
        .lines()
        .find_map(|line| line.strip_prefix("a=max-message-size:"))
        .ok_or(NetherNetError::InvalidMaxMessageSize)?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(NetherNetError::InvalidMaxMessageSize);
    }
    let size = value
        .parse::<u32>()
        .map_err(|_| NetherNetError::InvalidMaxMessageSize)?;
    if size <= 1 {
        return Err(NetherNetError::InvalidMaxMessageSize);
    }
    Ok(local.min((size - 1) as usize).min(TRANSPORT_MAX_PAYLOAD))
}

/// Advertise the SCTP ceiling using `conn.go:description.encode` semantics,
/// capped to webrtc-rs's fixed receive buffer (see `TRANSPORT_MAX_PAYLOAD`).
pub(crate) fn local_sdp(sdp: String) -> String {
    let mut result = String::with_capacity(sdp.len() + 28);
    let mut found = false;
    for line in sdp.lines() {
        if line.starts_with("a=max-message-size:") {
            result.push_str("a=max-message-size:65535\r\n");
            found = true;
        } else {
            result.push_str(line);
            result.push_str("\r\n");
        }
    }
    if !found {
        result.push_str("a=max-message-size:65535\r\n");
    }
    result
}

/// Validate before queuing or casting the countdown (`conn.go:Conn.Send`).
fn segment_count(size: usize, reliable: bool, payload: usize) -> Result<usize, NetherNetError> {
    if payload == 0 || payload > TRANSPORT_MAX_PAYLOAD {
        return Err(NetherNetError::InvalidConfiguration(
            "invalid segment payload size",
        ));
    }
    if !reliable && size > payload {
        return Err(NetherNetError::MessageTooLarge {
            size,
            limit: payload,
        });
    }
    let count = if size == 0 {
        0
    } else {
        (size - 1) / payload + 1
    };
    if count > 255 {
        return Err(NetherNetError::TooManySegments { count });
    }
    Ok(count)
}

async fn deliver_message(
    tx: &mpsc::Sender<Result<Message, NetherNetError>>,
    data: Bytes,
    reliable: bool,
    max_payload: usize,
) {
    let item = if data.len() > max_payload.saturating_add(1) {
        Err(NetherNetError::MessageTooLarge {
            size: data.len(),
            limit: max_payload.saturating_add(1),
        })
    } else if data.len() < 2 {
        Err(NetherNetError::InvalidPacket)
    } else {
        Ok(Message::new(data, reliable))
    };
    // Like message.go:wrapDataChannel, preserve reliable fragments under pressure.
    // Await in the callback instead of spawning unbounded pending send tasks.
    let _ = tx.send(item).await;
}

/// Register early on each accepted channel; retain the receiver for the stream.
pub(crate) fn install_message_handler(
    dc: &Arc<RTCDataChannel>,
    tx: mpsc::Sender<Result<Message, NetherNetError>>,
    reliable: bool,
) {
    let close_tx = tx.clone();
    dc.on_close(Box::new(move || {
        let tx = close_tx.clone();
        Box::pin(async move {
            let _ = tx.send(Err(NetherNetError::ConnectionClosed)).await;
        })
    }));
    dc.on_message(Box::new(move |msg| {
        let tx = tx.clone();
        Box::pin(
            async move { deliver_message(&tx, msg.data, reliable, TRANSPORT_MAX_PAYLOAD).await },
        )
    }));
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
    incoming_rx: mpsc::Receiver<Result<Message, NetherNetError>>,
    outbound_tx: PollSender<OutboundMessage>,
    pending_send: Option<oneshot::Receiver<Result<(), NetherNetError>>>,
    reassembly: ReassemblyState,
    max_message_size: usize,
    terminal: bool,
    sender_task: tokio::task::JoinHandle<()>,
    channels: [Arc<RTCDataChannel>; 2],
    peer: Option<Arc<webrtc::peer_connection::RTCPeerConnection>>,
    remote_identity: Option<crate::identity::VerifiedIdentity>,
}

struct OutboundMessage {
    message: Message,
    completion: oneshot::Sender<Result<(), NetherNetError>>,
}

/// Await actual SCTP drain, not merely the application's bounded send queue.
/// The low-buffer callback is the normal wakeup. Periodic state checks ensure a
/// remotely closed channel cannot strand a waiter (buffered_amount is not reset
/// by webrtc-rs on close).
async fn drain_channel(channel: &RTCDataChannel, drained: &Notify) -> Result<(), NetherNetError> {
    loop {
        if channel.ready_state()
            != webrtc::data_channel::data_channel_state::RTCDataChannelState::Open
        {
            return Err(NetherNetError::ConnectionClosed);
        }
        if channel.buffered_amount().await == 0 {
            return Ok(());
        }
        tokio::select! {
            _ = drained.notified() => {},
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
        }
    }
}

async fn send_message(
    channel: &RTCDataChannel,
    item: Message,
    max_len: usize,
    drained: &Notify,
) -> Result<(), NetherNetError> {
    let count = segment_count(item.buffer.len(), item.reliable, max_len)?;
    // Empty sends write no frames, matching conn.go:Conn.Send.
    for (index, fragment) in item.buffer.chunks(max_len).enumerate() {
        let mut frame = BytesMut::with_capacity(fragment.len() + 1);
        frame.extend_from_slice(&[(count - 1 - index) as u8]);
        frame.extend_from_slice(fragment);
        channel.send(&frame.freeze()).await?;
    }
    drain_channel(channel, drained).await
}

struct ReassemblyBuffer {
    remaining: u8,
    data: BytesMut,
    started_at: Instant,
}

struct ReassemblyState {
    buffer: Option<ReassemblyBuffer>,
    limit: usize,
    timeout: Duration,
}

impl ReassemblyState {
    fn new(limit: usize, timeout: Duration) -> Self {
        Self {
            buffer: None,
            limit,
            timeout,
        }
    }

    /// Header and strict countdown follow `message.go:parseMessage` and
    /// `dataChannel.handleMessage`. Size and time limits are local hardening.
    fn receive(
        &mut self,
        data: Bytes,
        reliable: bool,
        max_payload: usize,
    ) -> Result<Option<Message>, NetherNetError> {
        if data.len() < 2 {
            return Err(NetherNetError::InvalidPacket);
        }
        if data.len() - 1 > max_payload {
            return Err(NetherNetError::MessageTooLarge {
                size: data.len() - 1,
                limit: max_payload,
            });
        }
        let remaining = data[0];
        let payload = data.slice(1..);
        if !reliable {
            if remaining != 0 {
                return Err(NetherNetError::InvalidSegmentCount {
                    expected: 0,
                    actual: remaining,
                });
            }
            return Ok(Some(Message::unreliable(payload)));
        }
        if let Some(buffer) = self.buffer.as_mut() {
            if buffer.started_at.elapsed() > self.timeout {
                return Err(NetherNetError::ReassemblyTimeout);
            }
            let expected = buffer.remaining - 1;
            if remaining != expected {
                return Err(NetherNetError::InvalidSegmentCount {
                    expected,
                    actual: remaining,
                });
            }
            if payload.len() > self.limit.saturating_sub(buffer.data.len()) {
                return Err(NetherNetError::MessageTooLarge {
                    size: buffer.data.len().saturating_add(payload.len()),
                    limit: self.limit,
                });
            }
            buffer.data.extend_from_slice(&payload);
            buffer.remaining = remaining;
            if remaining == 0 {
                return Ok(Some(Message::reliable(
                    self.buffer.take().unwrap().data.freeze(),
                )));
            }
            return Ok(None);
        }
        if payload.len() > self.limit {
            return Err(NetherNetError::MessageTooLarge {
                size: payload.len(),
                limit: self.limit,
            });
        }
        if remaining == 0 {
            return Ok(Some(Message::reliable(payload)));
        }
        self.buffer = Some(ReassemblyBuffer {
            remaining,
            data: BytesMut::from(payload.as_ref()),
            started_at: Instant::now(),
        });
        Ok(None)
    }
}

impl NetherNetStream {
    /// Consume the receiver belonging to handlers registered before negotiation.
    pub(crate) fn new_with_receiver(
        reliable_dc: Arc<RTCDataChannel>,
        unreliable_dc: Arc<RTCDataChannel>,
        incoming_tx: mpsc::Sender<Result<Message, NetherNetError>>,
        incoming_rx: mpsc::Receiver<Result<Message, NetherNetError>>,
        config: NetherNetStreamConfig,
    ) -> Self {
        // A single maximum-sized packet may be queued, bounding outbound storage.
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundMessage>(1);
        let r_dc = reliable_dc.clone();
        let u_dc = unreliable_dc.clone();
        let max_len = config.max_message_size;
        let validation = config.validate();
        let sender_task = tokio::spawn(async move {
            if let Err(error) = validation {
                let _ = incoming_tx.send(Err(error)).await;
                return;
            }
            let notifications = [Arc::new(Notify::new()), Arc::new(Notify::new())];
            for (channel, notification) in [(&r_dc, &notifications[0]), (&u_dc, &notifications[1])]
            {
                let notification = notification.clone();
                channel.set_buffered_amount_low_threshold(0).await;
                channel
                    .on_buffered_amount_low(Box::new(move || {
                        let notification = notification.clone();
                        Box::pin(async move {
                            notification.notify_one();
                        })
                    }))
                    .await;
            }
            while let Some(OutboundMessage {
                message,
                completion,
            }) = outbound_rx.recv().await
            {
                let (channel, drained) = if message.reliable {
                    (&r_dc, &notifications[0])
                } else {
                    (&u_dc, &notifications[1])
                };
                let result = send_message(channel, message, max_len, drained).await;
                let failed = result.is_err();
                let _ = completion.send(result);
                if failed {
                    // The specific non-cloneable transport error belongs to the
                    // sending future; readers also learn that the stream ended.
                    let _ = incoming_tx
                        .send(Err(NetherNetError::ConnectionClosed))
                        .await;
                    return;
                }
            }
        });
        Self {
            incoming_rx,
            outbound_tx: PollSender::new(outbound_tx),
            pending_send: None,
            reassembly: ReassemblyState::new(config.max_reassembly_size, config.reassembly_timeout),
            max_message_size: config.max_message_size,
            terminal: false,
            sender_task,
            channels: [reliable_dc, unreliable_dc],
            peer: None,
            remote_identity: None,
        }
    }

    /// Connect after installing incoming signaling routing.
    /// Use `NetherNetDialer` directly when supplying a local signed identity.
    pub async fn connect<S: SignalingChannel + 'static>(
        network_id: String,
        signaling: S,
    ) -> Result<Self, NetherNetError> {
        Self::connect_with_config(network_id, signaling, NetherNetStreamConfig::default()).await
    }

    /// Connect using the configured timeout, ICE servers and stream bounds.
    /// The signaling subscription is established before the offer is sent.
    pub async fn connect_with_config<S: SignalingChannel + 'static>(
        network_id: String,
        signaling: S,
        config: NetherNetStreamConfig,
    ) -> Result<Self, NetherNetError> {
        config.validate()?;
        let dialer = crate::dialer::NetherNetDialer::connect_with_signaling(
            signaling,
            config.into_dialer_config(),
        );
        dialer.dial(network_id).await
    }

    pub(crate) fn attach_peer(&mut self, peer: Arc<webrtc::peer_connection::RTCPeerConnection>) {
        self.peer = Some(peer);
    }

    /// Remote key-possession proof bound to DTLS. Client JWT authenticity must
    /// additionally be established by a verifier or Login's GameServerToken/cpk binding.
    pub fn remote_identity(&self) -> Option<&crate::identity::VerifiedIdentity> {
        self.remote_identity.as_ref()
    }

    pub(crate) fn set_remote_identity(
        &mut self,
        identity: Option<crate::identity::VerifiedIdentity>,
    ) {
        self.remote_identity = identity;
    }

    fn poll_send_completion(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), NetherNetError>> {
        let Some(completion) = self.pending_send.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let result = match Pin::new(completion).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(result)) => result,
            Poll::Ready(Err(_)) => Err(NetherNetError::ConnectionClosed),
        };
        self.pending_send = None;
        if result.is_err() {
            self.terminate();
        }
        Poll::Ready(result)
    }

    fn terminate(&mut self) {
        if self.terminal {
            return;
        }
        self.terminal = true;
        self.incoming_rx.close();
        self.reassembly.buffer = None;
        self.outbound_tx.close();
        self.sender_task.abort();
        let channels = self.channels.clone();
        let peer = self.peer.take();
        // Channel callbacks may hold internal locks; close outside the callback.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for channel in channels {
                    let _ = channel.close().await;
                }
                if let Some(peer) = peer {
                    let _ = peer.close().await;
                }
            });
        }
    }
}

impl Stream for NetherNetStream {
    type Item = Result<Message, NetherNetError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminal {
            return Poll::Ready(None);
        }
        loop {
            match self.incoming_rx.poll_recv(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    match self
                        .reassembly
                        .receive(msg.buffer, msg.reliable, TRANSPORT_MAX_PAYLOAD)
                    {
                        Ok(Some(message)) => return Poll::Ready(Some(Ok(message))),
                        Ok(None) => continue,
                        Err(error) => {
                            self.terminate();
                            return Poll::Ready(Some(Err(error)));
                        }
                    }
                }
                Poll::Ready(Some(Err(error))) => {
                    self.terminate();
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(None) => {
                    self.terminate();
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl Sink<Message> for NetherNetStream {
    type Error = NetherNetError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        match self.poll_send_completion(cx) {
            Poll::Ready(Ok(())) => {}
            result => return result,
        }
        self.outbound_tx
            .poll_ready_unpin(cx)
            .map_err(|_| NetherNetError::ConnectionClosed)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        if self.terminal {
            return Err(NetherNetError::ConnectionClosed);
        }
        segment_count(item.buffer.len(), item.reliable, self.max_message_size)?;
        if self.pending_send.is_some() {
            return Err(NetherNetError::InvalidConfiguration(
                "poll_ready required before start_send",
            ));
        }
        let (completion, receiver) = oneshot::channel();
        self.outbound_tx
            .start_send_unpin(OutboundMessage {
                message: item,
                completion,
            })
            .map_err(|_| NetherNetError::ConnectionClosed)?;
        self.pending_send = Some(receiver);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        self.poll_send_completion(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Ok(()));
        }
        match self.poll_send_completion(cx) {
            Poll::Ready(Ok(())) => {
                self.terminate();
                Poll::Ready(Ok(()))
            }
            result => result,
        }
    }
}

impl Drop for NetherNetStream {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn test_stream(
        reliable: Arc<RTCDataChannel>,
        unreliable: Arc<RTCDataChannel>,
        config: NetherNetStreamConfig,
    ) -> NetherNetStream {
        config.validate().unwrap();
        let (tx, rx) = mpsc::channel(config.incoming_channel_capacity);
        install_message_handler(&reliable, tx.clone(), true);
        install_message_handler(&unreliable, tx.clone(), false);
        NetherNetStream::new_with_receiver(reliable, unreliable, tx, rx, config)
    }

    struct QuietSignaling {
        _tx: mpsc::Sender<crate::Signal>,
        rx: tokio::sync::Mutex<Option<mpsc::Receiver<crate::Signal>>>,
        wait_credentials: bool,
    }

    impl QuietSignaling {
        fn new(wait_credentials: bool) -> Self {
            let (tx, rx) = mpsc::channel(1);
            Self {
                _tx: tx,
                rx: tokio::sync::Mutex::new(Some(rx)),
                wait_credentials,
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::Signaling for QuietSignaling {
        async fn signal(&self, _: crate::Signal) -> anyhow::Result<()> {
            anyhow::bail!("negotiation must fail before sending an offer")
        }
        async fn credentials(&self) -> anyhow::Result<Option<crate::Credentials>> {
            if self.wait_credentials {
                std::future::pending().await
            } else {
                Ok(None)
            }
        }
        fn network_id(&self) -> String {
            "local".into()
        }
    }

    #[async_trait::async_trait]
    impl SignalingChannel for QuietSignaling {
        async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<crate::Signal>> {
            self.rx.lock().await.take()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn configured_deadline_covers_credential_wait() {
        let timeout = Duration::from_millis(123);
        let started = tokio::time::Instant::now();
        let result = NetherNetStream::connect_with_config(
            "remote".into(),
            QuietSignaling::new(true),
            NetherNetStreamConfig {
                connection_timeout: timeout,
                ..Default::default()
            },
        )
        .await;
        assert!(matches!(result, Err(NetherNetError::NegotiationTimeout)));
        assert_eq!(started.elapsed(), timeout);
    }

    #[tokio::test]
    async fn configured_ice_servers_reach_peer_validation() {
        let result = NetherNetStream::connect_with_config(
            "remote".into(),
            QuietSignaling::new(false),
            NetherNetStreamConfig {
                ice_servers: vec![RTCIceServer {
                    urls: vec!["invalid://host".into()],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .await;
        assert!(
            matches!(result, Err(NetherNetError::WebRTC(_))),
            "configured invalid ICE URL was ignored"
        );
    }

    #[test]
    fn rejects_invalid_configuration() {
        for size in [0, 65535, 262144, usize::MAX] {
            let config = NetherNetStreamConfig {
                max_message_size: size,
                ..Default::default()
            };
            assert!(config.validate().is_err());
        }
        let config = NetherNetStreamConfig {
            incoming_channel_capacity: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn segment_count_never_wraps() {
        assert_eq!(segment_count(255 * 3, true, 3).unwrap(), 255);
        assert!(matches!(
            segment_count(255 * 3 + 1, true, 3),
            Err(NetherNetError::TooManySegments { .. })
        ));
        assert_eq!(segment_count(0, true, 3).unwrap(), 0);
        assert!(matches!(
            segment_count(4, false, 3),
            Err(NetherNetError::MessageTooLarge { .. })
        ));
        assert!(segment_count(1, true, 0).is_err());
    }

    #[test]
    fn receives_both_channel_headers_and_strict_countdown() {
        let mut state = ReassemblyState::new(100, Duration::from_secs(1));
        assert!(
            state
                .receive(Bytes::from_static(&[2, 1]), true, 10)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            state.receive(Bytes::from_static(&[2, 2]), true, 10),
            Err(NetherNetError::InvalidSegmentCount {
                expected: 1,
                actual: 2
            })
        ));
        let mut state = ReassemblyState::new(100, Duration::from_secs(1));
        assert!(
            state
                .receive(Bytes::from_static(&[1, 1]), true, 10)
                .unwrap()
                .is_none()
        );
        let unreliable = state
            .receive(Bytes::from_static(&[0, 9]), false, 10)
            .unwrap()
            .unwrap();
        assert_eq!(unreliable.buffer.as_ref(), &[9]);
        assert!(!unreliable.reliable);
        assert_eq!(
            state
                .receive(Bytes::from_static(&[0, 2]), true, 10)
                .unwrap()
                .unwrap()
                .buffer
                .as_ref(),
            &[1, 2]
        );
        assert!(
            state
                .receive(Bytes::from_static(&[1, 9]), false, 10)
                .is_err()
        );
    }

    #[test]
    fn receives_exactly_255_segments_without_overflow() {
        let mut state = ReassemblyState::new(255, Duration::from_secs(1));
        for remaining in (1..255u8).rev() {
            assert!(
                state
                    .receive(Bytes::from(vec![remaining, 7]), true, 1)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(
            state
                .receive(Bytes::from_static(&[0, 7]), true, 1)
                .unwrap()
                .unwrap()
                .buffer
                .len(),
            255
        );
    }

    #[test]
    fn receive_limits_include_single_packets_and_timeout() {
        let mut state = ReassemblyState::new(1, Duration::ZERO);
        assert!(
            state
                .receive(Bytes::from_static(&[0, 1, 2]), true, 10)
                .is_err()
        );
        assert!(state.receive(Bytes::from_static(&[0]), true, 10).is_err());
        assert!(
            state
                .receive(Bytes::from_static(&[0, 1, 2]), false, 1)
                .is_err()
        );
        assert!(
            state
                .receive(Bytes::from_static(&[1, 1]), true, 10)
                .unwrap()
                .is_none()
        );
        state.buffer.as_mut().unwrap().started_at = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            state.receive(Bytes::from_static(&[0, 2]), true, 10),
            Err(NetherNetError::ReassemblyTimeout)
        ));
    }

    #[tokio::test]
    async fn callback_delivery_backpressures_instead_of_dropping() {
        let (tx, mut rx) = mpsc::channel(1);
        deliver_message(&tx, Bytes::from_static(&[0, 1]), true, 10).await;
        let delivery = deliver_message(&tx, Bytes::from_static(&[0, 2]), true, 10);
        tokio::pin!(delivery);
        assert!(futures::poll!(&mut delivery).is_pending());
        assert_eq!(rx.recv().await.unwrap().unwrap().buffer.as_ref(), &[0, 1]);
        delivery.await;
        assert_eq!(rx.recv().await.unwrap().unwrap().buffer.as_ref(), &[0, 2]);
        deliver_message(&tx, Bytes::from_static(&[0, 1, 2]), false, 1).await;
        assert!(matches!(
            rx.recv().await.unwrap(),
            Err(NetherNetError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn negotiates_required_sctp_limit() {
        assert_eq!(
            negotiate_max_message_size("v=0\r\na=max-message-size:65536\r\n", 262143).unwrap(),
            65534
        );
        for sdp in [
            "v=0\r\n",
            "a=max-message-size:0\r\n",
            "a=max-message-size:1\r\n",
            "a=max-message-size:4294967296\r\n",
            "a=max-message-size:wat\r\n",
        ] {
            assert!(negotiate_max_message_size(sdp, 262143).is_err());
        }
        assert_eq!(
            local_sdp("v=0\r\na=max-message-size:65536\r\n".into()),
            "v=0\r\na=max-message-size:65535\r\n"
        );
    }
}

#[cfg(test)]
mod loopback_tests {
    use super::tests::test_stream;
    use super::*;
    use futures::StreamExt;
    use webrtc::{
        api::{APIBuilder, setting_engine::SettingEngine},
        data_channel::data_channel_init::RTCDataChannelInit,
        peer_connection::{RTCPeerConnection, configuration::RTCConfiguration},
    };

    async fn peer() -> Arc<RTCPeerConnection> {
        let mut settings = SettingEngine::default();
        settings.set_include_loopback_candidate(true);
        settings.set_ip_filter(Box::new(|ip| ip.is_loopback()));
        Arc::new(
            APIBuilder::new()
                .with_setting_engine(settings)
                .build()
                .new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn flush_and_close_wait_for_sender_completion() {
        let pc = peer().await;
        let reliable = pc
            .create_data_channel("ReliableDataChannel", None)
            .await
            .unwrap();
        let unreliable = pc
            .create_data_channel("UnreliableDataChannel", None)
            .await
            .unwrap();
        let mut stream = test_stream(reliable, unreliable, NetherNetStreamConfig::default());
        stream.attach_peer(pc.clone());
        // Drive the real Sink state machine while holding its completion channel,
        // without scheduling or timing assumptions about network writes.
        let (completion, receiver) = oneshot::channel();
        stream.pending_send = Some(receiver);
        {
            let flush = stream.flush();
            tokio::pin!(flush);
            assert!(futures::poll!(&mut flush).is_pending());
        }
        {
            let close = stream.close();
            tokio::pin!(close);
            assert!(futures::poll!(&mut close).is_pending());
        }
        assert!(!stream.terminal);
        completion.send(Ok(())).unwrap();
        stream.close().await.unwrap();
        assert!(stream.terminal);
        pc.close().await.unwrap();
    }

    #[tokio::test]
    async fn send_reports_actual_transport_failure() {
        let pc = peer().await;
        let reliable = pc
            .create_data_channel("ReliableDataChannel", None)
            .await
            .unwrap();
        let unreliable = pc
            .create_data_channel("UnreliableDataChannel", None)
            .await
            .unwrap();
        let mut stream = test_stream(reliable, unreliable, NetherNetStreamConfig::default());
        stream.attach_peer(pc.clone());
        let error = stream
            .send(Message::reliable(Bytes::from_static(b"closed")))
            .await
            .unwrap_err();
        assert!(matches!(error, NetherNetError::WebRTC(_)));
        pc.close().await.unwrap();
    }

    #[tokio::test]
    async fn real_loopback_sctp_fragmentation_and_unreliable_header() {
        tokio::time::timeout(Duration::from_secs(30), async {
            let client = peer().await;
            let server = peer().await;
            let (accepted_tx, mut accepted_rx) = mpsc::channel(2);
            server.on_data_channel(Box::new(move |dc| {
                let tx = accepted_tx.clone();
                let opened = dc.clone();
                dc.on_open(Box::new(move || {
                    let tx = tx.clone();
                    let dc = opened.clone();
                    Box::pin(async move {
                        tx.send(dc).await.unwrap();
                    })
                }));
                Box::pin(async {})
            }));
            let reliable = client
                .create_data_channel(
                    "ReliableDataChannel",
                    Some(RTCDataChannelInit {
                        ordered: Some(true),
                        ..Default::default()
                    }),
                )
                .await
                .unwrap();
            let unreliable = client
                .create_data_channel(
                    "UnreliableDataChannel",
                    Some(RTCDataChannelInit {
                        ordered: Some(false),
                        max_retransmits: Some(0),
                        ..Default::default()
                    }),
                )
                .await
                .unwrap();
            let (opened_tx, mut opened_rx) = mpsc::channel(2);
            for dc in [&reliable, &unreliable] {
                let tx = opened_tx.clone();
                dc.on_open(Box::new(move || {
                    let tx = tx.clone();
                    Box::pin(async move {
                        tx.send(()).await.unwrap();
                    })
                }));
            }

            let offer = client.create_offer(None).await.unwrap();
            let mut complete = client.gathering_complete_promise().await;
            client.set_local_description(offer).await.unwrap();
            complete.recv().await;
            server
                .set_remote_description(client.local_description().await.unwrap())
                .await
                .unwrap();
            let answer = server.create_answer(None).await.unwrap();
            let mut complete = server.gathering_complete_promise().await;
            server.set_local_description(answer).await.unwrap();
            complete.recv().await;
            client
                .set_remote_description(server.local_description().await.unwrap())
                .await
                .unwrap();
            opened_rx.recv().await.unwrap();
            opened_rx.recv().await.unwrap();
            let first = accepted_rx.recv().await.unwrap();
            let second = accepted_rx.recv().await.unwrap();
            let (remote_r, remote_u) = if first.label() == "ReliableDataChannel" {
                (first, second)
            } else {
                (second, first)
            };
            // Exercise fragmentation across the actual webrtc-rs 65535-byte
            // frame receive ceiling, not only small synthetic fragments.
            let config = NetherNetStreamConfig::default();
            let outgoing_small = NetherNetStreamConfig {
                max_message_size: 16383,
                ..config.clone()
            };
            let mut a = test_stream(reliable, unreliable, outgoing_small);
            let mut b = test_stream(remote_r, remote_u, config);
            a.attach_peer(client.clone());
            b.attach_peer(server.clone());
            let payload = Bytes::from((0..140000).map(|n| n as u8).collect::<Vec<_>>());
            a.send(Message::reliable(payload.clone())).await.unwrap();
            let received = b.next().await.unwrap().unwrap();
            assert!(received.reliable);
            assert_eq!(received.buffer, payload);
            b.send(Message::unreliable(Bytes::from_static(b"reply")))
                .await
                .unwrap();
            let received = a.next().await.unwrap().unwrap();
            assert!(!received.reliable);
            assert_eq!(received.buffer.as_ref(), b"reply");
            // Remote receive limits constrain only outbound segments: a peer
            // advertising 16384 may still send us a 20001-byte frame.
            let asymmetric = Bytes::from(vec![7; 20000]);
            b.send(Message::reliable(asymmetric.clone())).await.unwrap();
            assert_eq!(a.next().await.unwrap().unwrap().buffer, asymmetric);
            // Closing immediately after send must not abort remaining fragments.
            let final_packet = Bytes::from(vec![9; 140000]);
            let expected = final_packet.clone();
            let (sent, received) = tokio::join!(
                async {
                    a.send(Message::reliable(final_packet)).await.unwrap();
                    a.close().await.unwrap();
                },
                async { b.next().await.unwrap().unwrap() },
            );
            assert_eq!(sent, ());
            assert_eq!(received.buffer, expected);
            drop(a);
            drop(b);
            client.close().await.unwrap();
            server.close().await.unwrap();
        })
        .await
        .expect("loopback SCTP exchange timed out");
    }
}
