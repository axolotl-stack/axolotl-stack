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

    /// The P-384 key the peer proved it holds at the transport layer, if any
    /// (NetherNet: the SDP identity assertion bound to DTLS). With Bedrock
    /// encryption off, an online login is authenticated only when its
    /// identity key equals this key.
    fn proven_peer_key(&self) -> Option<p384::PublicKey> {
        None
    }
}
