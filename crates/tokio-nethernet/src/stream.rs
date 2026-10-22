use bytes::Bytes;
use futures::{Sink, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::time::Duration;

use crate::error::NetherNetError;
use crate::framing::MAX_SEGMENT_PAYLOAD;
use crate::signaling::{IceServer, SignalingChannel};
use crate::transport::Connection;

/// Local bounded-state policy, not a wire constant.
const DEFAULT_MAX_REASSEMBLY_SIZE: usize = MAX_SEGMENT_PAYLOAD * 255;

/// Default reassembly timeout (30 seconds).
const DEFAULT_REASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for `NetherNetStream`.
#[derive(Debug, Clone)]
pub struct NetherNetStreamConfig {
    /// Outbound segment payload ceiling (1..=262143); the remote
    /// `max-message-size - 1` also applies. Does not limit inbound segments.
    pub max_message_size: usize,
    /// Timeout for connection establishment.
    pub connection_timeout: Duration,
    /// Explicit ICE servers; nonempty values override signaling credentials.
    pub ice_servers: Vec<IceServer>,
    /// Maximum size of the reassembly buffer in bytes.
    /// Prevents OOM attacks from fragmented messages.
    pub max_reassembly_size: usize,
    /// Timeout for reassembling fragmented messages.
    /// Rejects subsequent fragments once this duration has elapsed.
    pub reassembly_timeout: Duration,
    /// Completed messages buffered before SCTP reads pause; the paused stream's
    /// receive window then applies backpressure to the peer.
    pub incoming_channel_capacity: usize,
}

impl Default for NetherNetStreamConfig {
    fn default() -> Self {
        Self {
            max_message_size: MAX_SEGMENT_PAYLOAD,
            // dial.go:Dialer.Dial and credentials.go:gatherOptions.
            connection_timeout: Duration::from_secs(15),
            ice_servers: Vec::new(),
            max_reassembly_size: DEFAULT_MAX_REASSEMBLY_SIZE,
            reassembly_timeout: DEFAULT_REASSEMBLY_TIMEOUT,
            // Bounded like message.go:wrapDataChannel's packet handoff; SCTP's
            // window holds everything beyond it.
            incoming_channel_capacity: 64,
        }
    }
}

impl NetherNetStreamConfig {
    pub(crate) fn validate(&self) -> Result<(), NetherNetError> {
        if self.max_message_size == 0 || self.max_message_size > MAX_SEGMENT_PAYLOAD {
            return Err(NetherNetError::InvalidConfiguration(
                "segment size must be 1..=262143",
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

/// A NetherNet connection: `Stream` of received messages and `Sink` for sends.
///
/// Sends are written to SCTP and the socket inline (no task hop) and complete
/// once SCTP accepted every segment, like go-nethernet `conn.go:Conn.Send`;
/// they pause while more than 1 MiB is unacknowledged. Graceful `close`
/// additionally waits until every queued byte is acknowledged. Dropping the
/// stream aborts the connection.
pub struct NetherNetStream {
    connection: Arc<Connection>,
    driver: tokio::task::JoinHandle<()>,
    remote_identity: Option<crate::identity::VerifiedIdentity>,
    terminal: bool,
}

impl NetherNetStream {
    pub(crate) fn new(
        connection: Arc<Connection>,
        driver: tokio::task::JoinHandle<()>,
        remote_identity: Option<crate::identity::VerifiedIdentity>,
    ) -> Self {
        Self {
            connection,
            driver,
            remote_identity,
            terminal: false,
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

    /// Remote key-possession proof bound to DTLS. Client JWT authenticity must
    /// additionally be established by a verifier or Login's GameServerToken/cpk binding.
    pub fn remote_identity(&self) -> Option<&crate::identity::VerifiedIdentity> {
        self.remote_identity.as_ref()
    }

    /// Smoothed SCTP round-trip time (`None` until the first measurement).
    pub fn rtt(&self) -> Option<std::time::Duration> {
        self.connection.rtt()
    }

    pub(crate) fn connection(&self) -> &Arc<Connection> {
        &self.connection
    }

    fn terminate(&mut self) {
        if self.terminal {
            return;
        }
        self.terminal = true;
        self.connection.close();
    }

    /// Map a contained failure to the stream's terminal error.
    fn finish<T>(
        &mut self,
        result: Result<Poll<Result<T, NetherNetError>>, NetherNetError>,
    ) -> Poll<Result<T, NetherNetError>> {
        match result {
            Ok(Poll::Ready(Err(error))) | Err(error) => {
                self.terminate();
                Poll::Ready(Err(error))
            }
            Ok(poll) => poll,
        }
    }
}

impl Stream for NetherNetStream {
    type Item = Result<Message, NetherNetError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.terminal {
            return Poll::Ready(None);
        }
        let polled = self
            .connection
            .with("receive", |session, now| session.poll_recv(now, cx));
        match polled {
            Ok(Poll::Ready(Some(Ok(message)))) => Poll::Ready(Some(Ok(message))),
            Ok(Poll::Ready(Some(Err(error)))) | Err(error) => {
                self.terminate();
                Poll::Ready(Some(Err(error)))
            }
            Ok(Poll::Ready(None)) => {
                self.terminate();
                Poll::Ready(None)
            }
            Ok(Poll::Pending) => Poll::Pending,
        }
    }
}

impl Sink<Message> for NetherNetStream {
    type Error = NetherNetError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        let polled = self
            .connection
            .with("send", |session, _| session.poll_send_ready(cx));
        self.finish(polled)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        if self.terminal {
            return Err(NetherNetError::ConnectionClosed);
        }
        let result = self.connection.with("send", |session, now| {
            (session.send(now, &item), session.is_finished())
        });
        match result {
            Ok((Ok(()), _)) => Ok(()),
            // Size violations reject this message only; transport failures end the stream.
            Ok((Err(error), finished)) => {
                if finished {
                    self.terminate();
                }
                Err(error)
            }
            Err(error) => {
                self.terminate();
                Err(error)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Err(NetherNetError::ConnectionClosed));
        }
        // Sends are handed to SCTP and the socket inline in `start_send`.
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.terminal {
            return Poll::Ready(Ok(()));
        }
        let polled = self
            .connection
            .with("close", |session, _| session.poll_drained(cx));
        match self.finish(polled) {
            Poll::Ready(Ok(())) => {
                self.terminate();
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl Drop for NetherNetStream {
    fn drop(&mut self) {
        self.terminate();
        self.driver.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connected_pair, connected_pair_with};
    use futures::{SinkExt, StreamExt};
    use tokio::sync::mpsc;

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
    async fn configured_ice_servers_override_signaling_credential_wait() {
        // Credentials never resolve: reaching the offer proves they were skipped.
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            NetherNetStream::connect_with_config(
                "remote".into(),
                QuietSignaling::new(true),
                NetherNetStreamConfig {
                    ice_servers: vec![IceServer {
                        username: String::new(),
                        password: String::new(),
                        urls: vec!["stun:127.0.0.1:9".into()],
                    }],
                    ..Default::default()
                },
            ),
        )
        .await
        .expect("configured ICE servers must not wait for signaling credentials");
        assert!(matches!(result, Err(NetherNetError::Signaling(_))));
    }

    #[test]
    fn rejects_invalid_configuration() {
        for size in [0, 262144, usize::MAX] {
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

    /// NETHERNET-04: rtc-sctp acknowledges a lone DATA chunk only after its
    /// 200 ms delayed-ACK timer. Sequential small sends must complete on SCTP
    /// acceptance; the peer application reads nothing until all sends
    /// completed, so no reply can carry an early acknowledgement.
    #[tokio::test]
    async fn small_reliable_sends_do_not_wait_for_delayed_sack() {
        const SENDS: usize = 20;
        const DELAYED_ACK: Duration = Duration::from_millis(200);
        tokio::time::timeout(Duration::from_secs(30), async {
            let (mut a, mut b, _listener) = connected_pair(NetherNetStreamConfig::default()).await;
            let started = std::time::Instant::now();
            for index in 0..SENDS {
                a.send(Message::reliable(Bytes::from(vec![index as u8; 32])))
                    .await
                    .unwrap();
            }
            let elapsed = started.elapsed();
            assert!(
                elapsed < DELAYED_ACK,
                "{SENDS} small sends took {elapsed:?}; send completion waits for SACKs"
            );
            for index in 0..SENDS {
                let received = b.next().await.unwrap().unwrap();
                assert!(received.reliable);
                assert_eq!(received.buffer.as_ref(), [index as u8; 32]);
            }
        })
        .await
        .expect("loopback exchange timed out");
    }

    #[tokio::test]
    async fn real_loopback_fragmentation_unreliable_header_and_drained_close() {
        tokio::time::timeout(Duration::from_secs(30), async {
            // Exercise multi-segment messages larger than one SCTP message.
            let outgoing_small = NetherNetStreamConfig {
                max_message_size: 16383,
                ..NetherNetStreamConfig::default()
            };
            let (mut a, mut b, _listener) = connected_pair(outgoing_small).await;
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
            // A single 262143-byte segment, the go-nethernet maximum.
            let largest = Bytes::from(vec![7; MAX_SEGMENT_PAYLOAD]);
            b.send(Message::reliable(largest.clone())).await.unwrap();
            assert_eq!(a.next().await.unwrap().unwrap().buffer, largest);
            // Unreliable messages are one segment.
            assert!(matches!(
                b.send(Message::unreliable(Bytes::from(vec![
                    0;
                    MAX_SEGMENT_PAYLOAD
                        + 1
                ])))
                .await,
                Err(NetherNetError::MessageTooLarge { .. })
            ));
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
            // go-nethernet close semantics: the channels reset and the ABORT
            // that follows the drained close end the peer at once (no ICE
            // timeout), as an error like go's `net.ErrClosed`.
            let closed_at = std::time::Instant::now();
            assert!(matches!(
                b.next().await,
                Some(Err(NetherNetError::ConnectionClosed))
            ));
            assert!(closed_at.elapsed() < Duration::from_secs(1));
            assert!(b.next().await.is_none());
            assert!(matches!(
                b.send(Message::reliable(Bytes::from_static(b"late"))).await,
                Err(NetherNetError::ConnectionClosed)
            ));
        })
        .await
        .expect("loopback exchange timed out");
    }

    /// A panic inside the transport poisons that connection only: every
    /// waiter gets `DependencyPanic`, the driver exits instead of re-entering
    /// rtc on its next tick, and the peer connection is unaffected.
    #[tokio::test]
    async fn contained_panic_poisons_the_connection() {
        tokio::time::timeout(Duration::from_secs(30), async {
            let (mut a, mut b, _listener) = connected_pair(NetherNetStreamConfig::default()).await;
            let error = a
                .connection
                .with("test", |_, _| -> () { panic!("rtc exploded") })
                .unwrap_err();
            assert!(matches!(
                error,
                NetherNetError::DependencyPanic {
                    context: "test",
                    ..
                }
            ));
            match a.next().await {
                Some(Err(NetherNetError::DependencyPanic { message, .. })) => {
                    assert_eq!(message, "rtc exploded")
                }
                other => panic!("expected DependencyPanic, got {other:?}"),
            }
            assert!(a.next().await.is_none());
            assert!(matches!(
                a.send(Message::reliable(Bytes::from_static(b"x"))).await,
                Err(NetherNetError::ConnectionClosed)
            ));
            while !a.driver.is_finished() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // Teardown is skipped: no rtc call after the panic.
            a.connection.close();
            assert!(
                a.connection
                    .with("check", |session, _| session.poll_timeout())
                    .unwrap()
                    .is_none()
            );
            drop(a);
            // The peer only sees its transport go quiet (no ABORT was sent).
            b.send(Message::reliable(Bytes::from_static(b"still writable")))
                .await
                .unwrap();
        })
        .await
        .expect("poisoned connection test timed out");
    }

    /// Reassembly failures keep their type for the reader (`message.go`
    /// rejects a countdown gap).
    #[tokio::test]
    async fn framing_errors_reach_the_reader_typed() {
        tokio::time::timeout(Duration::from_secs(30), async {
            let (a, mut b, _listener) = connected_pair(NetherNetStreamConfig::default()).await;
            a.connection
                .with("test", |session, now| {
                    session.write_raw_segment(now, true, &[2, b'x']);
                    session.write_raw_segment(now, true, &[0, b'y']);
                })
                .unwrap();
            match b.next().await {
                Some(Err(NetherNetError::InvalidSegmentCount { expected, actual })) => {
                    assert_eq!((expected, actual), (1, 0));
                }
                other => panic!("expected InvalidSegmentCount, got {other:?}"),
            }
        })
        .await
        .expect("framing error test timed out");
    }

    /// A certificate that does not match the signaled fingerprint fails the
    /// DTLS handshake on both sides (`dtlstransport.go:validateFingerPrint`).
    #[tokio::test]
    async fn fingerprint_mismatch_rejects_the_connection() {
        fn tamper(signal: &mut crate::Signal) {
            if signal.typ == crate::SignalType::Offer {
                let start = signal.data.find("a=fingerprint:sha-256 ").unwrap() + 22;
                let digit = if signal.data.as_bytes()[start] == b'0' {
                    "1"
                } else {
                    "0"
                };
                signal.data.replace_range(start..start + 1, digit);
            }
        }
        let (server, client) = crate::test_support::signaling_pair_rewriting(false, Some(tamper));
        let mut listener = crate::NetherNetListener::bind_with_signaling(
            server,
            crate::NetherNetListenerConfig {
                transport: crate::TransportConfig::loopback_v4(),
                allow_identityless_client: true,
                ..Default::default()
            },
        );
        let dialer = crate::NetherNetDialer::connect_with_signaling(
            client,
            crate::NetherNetDialerConfig {
                transport: crate::TransportConfig::loopback_v4(),
                allow_identityless_server: true,
                connection_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        );
        let started = std::time::Instant::now();
        let dialed = dialer.dial("1".into()).await;
        assert!(dialed.is_err(), "tampered fingerprint must not connect");
        // Rejected by the handshake (alert), not by the negotiation deadline.
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "{:?}",
            started.elapsed()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_err()
        );
    }

    /// Trickle signaling: offers/answers carry no candidates and every
    /// candidate travels as `CANDIDATEADD` (conn.go:trickleCandidates).
    #[tokio::test]
    async fn trickled_candidates_establish_the_connection() {
        tokio::time::timeout(Duration::from_secs(30), async {
            let (mut a, mut b, _listener) =
                connected_pair_with(NetherNetStreamConfig::default(), true).await;
            a.send(Message::reliable(Bytes::from_static(b"trickled")))
                .await
                .unwrap();
            assert_eq!(
                b.next().await.unwrap().unwrap().buffer.as_ref(),
                b"trickled"
            );
        })
        .await
        .expect("trickled negotiation timed out");
    }

    #[tokio::test]
    async fn bounded_inbound_queue_backpressures_through_sctp_window() {
        tokio::time::timeout(Duration::from_secs(30), async {
            let config = NetherNetStreamConfig {
                incoming_channel_capacity: 1,
                ..NetherNetStreamConfig::default()
            };
            let (mut a, mut b, _listener) = connected_pair(config).await;
            for index in 0..200u32 {
                a.send(Message::reliable(Bytes::from(
                    index.to_be_bytes().repeat(256),
                )))
                .await
                .unwrap();
            }
            for index in 0..200u32 {
                let received = b.next().await.unwrap().unwrap();
                assert_eq!(&received.buffer[..4], index.to_be_bytes());
            }
        })
        .await
        .expect("paused channel was never resumed");
    }
}
