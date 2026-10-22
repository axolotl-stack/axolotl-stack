//! RakNet adapter over `tokio_raknet::RaknetStream`.

use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Sink;
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::transport::{Message, RaknetStream};

use super::{Datagram, Framing, Transmit, Transport};

/// A RakNet connection carrying Bedrock game frames (`0xFE` messages).
pub struct RakNetTransport {
    inner: RaknetStream,
}

impl RakNetTransport {
    pub fn new(stream: RaknetStream) -> Self {
        Self { inner: stream }
    }

    pub fn into_inner(self) -> RaknetStream {
        self.inner
    }
}

impl Transport for RakNetTransport {
    type Error = tokio_raknet::RaknetError;

    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        <RaknetStream as Sink<Message>>::poll_ready(Pin::new(&mut self.inner), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> Result<(), Self::Error> {
        let reliability = if message.reliable {
            Reliability::ReliableOrdered
        } else {
            Reliability::Unreliable
        };
        <RaknetStream as Sink<Message>>::start_send(
            Pin::new(&mut self.inner),
            Message::new(message.payload).reliability(reliability),
        )
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <RaknetStream as Sink<Message>>::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <RaknetStream as Sink<Message>>::poll_close(Pin::new(&mut self.inner), cx)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Datagram, Self::Error>>> {
        Pin::new(&mut self.inner)
            .poll_recv_message(cx)
            .map(|message| {
                message.map(|message| {
                    message.map(|message| Datagram::SplitFirst {
                        first: message.id,
                        rest: message.payload,
                    })
                })
            })
    }

    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer_addr()
    }
}
