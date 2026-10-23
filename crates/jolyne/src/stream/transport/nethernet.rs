//! NetherNet (WebRTC) adapter over `tokio_nethernet::NetherNetStream`.

use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Sink, Stream};
use tokio_nethernet::{Message, NetherNetError, NetherNetStream};

use super::{Datagram, Framing, Transmit, Transport};

/// A NetherNet connection. Batches carry no `0xFE` prefix and DTLS already
/// encrypts the channel, so Bedrock encryption is normally off.
pub struct NetherNetTransport {
    stream: NetherNetStream,
    /// WebRTC has no peer socket address; this is a logging label.
    addr: SocketAddr,
}

impl NetherNetTransport {
    /// Wraps `stream` with an unspecified (`0.0.0.0:0`) logging address.
    pub fn new(stream: NetherNetStream) -> Self {
        Self::with_addr(stream, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
    }

    /// Wraps `stream`, reporting `addr` as its peer address for logging.
    pub fn with_addr(stream: NetherNetStream, addr: SocketAddr) -> Self {
        Self { stream, addr }
    }

    /// Returns the underlying NetherNet stream.
    pub fn into_inner(self) -> NetherNetStream {
        self.stream
    }
}

impl Transport for NetherNetTransport {
    type Error = NetherNetError;

    const FRAMING: Framing = Framing::Unprefixed;

    fn poll_send_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        <NetherNetStream as Sink<Message>>::poll_ready(Pin::new(&mut self.stream), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> Result<(), Self::Error> {
        <NetherNetStream as Sink<Message>>::start_send(
            Pin::new(&mut self.stream),
            Message::new(message.payload, message.reliable),
        )
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <NetherNetStream as Sink<Message>>::poll_flush(Pin::new(&mut self.stream), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <NetherNetStream as Sink<Message>>::poll_close(Pin::new(&mut self.stream), cx)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Self::Error>>> {
        Pin::new(&mut self.stream)
            .poll_next(cx)
            .map(|message| message.map(|message| message.map(|m| Datagram::Contiguous(m.buffer))))
    }

    fn peer_addr(&self) -> SocketAddr {
        self.addr
    }

    fn latency(&self) -> Option<std::time::Duration> {
        self.stream.rtt()
    }

    fn proven_peer_key(&self) -> Option<p384::PublicKey> {
        self.stream
            .remote_identity()
            .map(|identity| p384::PublicKey::from(&identity.public_key))
    }
}
