//! Message transports the async layer drives the protocol core over.
//!
//! A transport moves whole Bedrock batches. RakNet and NetherNet adapters are
//! provided; anything else (in-memory pipes, custom relays) implements
//! [`Transport`] directly.

#[cfg(feature = "nethernet")]
pub mod nethernet;
#[cfg(feature = "raknet")]
pub mod raknet;

#[cfg(feature = "nethernet")]
pub use nethernet::NetherNetTransport;
#[cfg(feature = "raknet")]
pub use raknet::RakNetTransport;

use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

pub use crate::proto::conn::{Datagram, Framing, Transmit};

/// A message-oriented transport with sink-style backpressure.
///
/// The contract mirrors `futures::Sink`: [`Transport::poll_send_ready`] must
/// return `Ready(Ok)` before each [`Transport::start_send`], and a message
/// handed to `start_send` is committed (it is never resubmitted).
pub trait Transport: Unpin + Send {
    /// Error returned by send, flush, close and receive operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// RakNet prefixes batches with `0xFE`; message transports do not.
    const FRAMING: Framing;

    /// Waits for capacity for one message.
    fn poll_send_ready(self: Pin<&mut Self>, cx: &mut Context<'_>)
    -> Poll<Result<(), Self::Error>>;

    /// Commits one message. Only call after `poll_send_ready` returned `Ready(Ok)`.
    fn start_send(self: Pin<&mut Self>, message: Transmit) -> Result<(), Self::Error>;

    /// Pushes committed messages towards the peer.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Gracefully closes the transport after committed messages drain.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// The next inbound message; `None` once the peer is gone.
    fn poll_recv(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Self::Error>>>;

    /// Peer address (a placeholder for WebRTC transports).
    fn peer_addr(&self) -> SocketAddr;

    /// Smoothed round-trip time, when the transport measures it.
    fn latency(&self) -> Option<Duration> {
        None
    }

    /// A handle reporting the bytes this transport has queued towards the
    /// peer, when it tracks them (RakNet: reliable data its session counts
    /// against the queue limit that disconnects the peer).
    fn send_queue_probe(&self) -> Option<SendQueueProbe> {
        None
    }

    /// The P-384 key the peer proved it holds at the transport layer, if any
    /// (NetherNet: the SDP identity assertion bound to DTLS). With Bedrock
    /// encryption off, an online login is authenticated only when its
    /// identity key equals this key.
    fn proven_peer_key(&self) -> Option<p384::PublicKey> {
        None
    }

    /// Whether a `poll_recv` error is the peer closing the connection
    /// normally (RakNet: its disconnection notification, which vanilla
    /// clients send on Save and Quit without a Bedrock `Disconnect`) rather
    /// than a failure. Receives report such an error as
    /// [`JolyneError::ConnectionClosed`](crate::JolyneError::ConnectionClosed).
    fn is_peer_close(_error: &Self::Error) -> bool {
        false
    }
}

/// Reads how many bytes a connection has queued towards its peer, without
/// access to the stream. Senders use it to stay under the transport's queue
/// limit.
#[derive(Clone)]
pub struct SendQueueProbe(std::sync::Arc<dyn Fn() -> usize + Send + Sync>);

impl SendQueueProbe {
    /// A probe reporting `queued_bytes()`.
    pub fn new(queued_bytes: impl Fn() -> usize + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(queued_bytes))
    }

    /// Bytes queued towards the peer now.
    pub fn queued_bytes(&self) -> usize {
        (self.0)()
    }
}

impl std::fmt::Debug for SendQueueProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SendQueueProbe")
            .field(&self.queued_bytes())
            .finish()
    }
}
