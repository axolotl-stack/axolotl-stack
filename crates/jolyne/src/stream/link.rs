//! A transport plus its codec state, outside the join typestate.

use std::time::{Duration, Instant};

use super::driver;
use super::slots::Seat;
use super::transport::Transport;
use crate::config::{ConnectionOptions, FlushMode};
use crate::error::JolyneError;
use crate::proto::conn::Connection;
use crate::proto::custom::CustomPacket;
use crate::proto::observe::ConnectionMetrics;
use crate::proto::raw::RawPacket;
use crate::valentine::{BorrowedMcpePacket, EnumsConnectionDisconnectFailReason, McpePacket};

/// Raw packet I/O over a transport: framing, compression and encryption
/// without phase rules. Obtained from [`super::BedrockStream::into_transport`]
/// (codec state carries over) or built fresh with [`BedrockTransport::new`].
pub struct BedrockTransport<T: Transport> {
    transport: T,
    conn: Connection,
    _seat: Seat,
}

impl<T: Transport> Drop for BedrockTransport<T> {
    fn drop(&mut self) {
        driver::flush_now(&mut self.transport, &mut self.conn);
    }
}

impl<T: Transport> BedrockTransport<T> {
    /// A link with no compression or encryption negotiated yet.
    pub fn new(transport: T, options: ConnectionOptions) -> Self {
        Self::from_parts(
            transport,
            Connection::new(T::FRAMING, options),
            Seat::Uncounted,
        )
    }

    pub(crate) fn from_parts(transport: T, conn: Connection, seat: Seat) -> Self {
        Self {
            transport,
            conn,
            _seat: seat,
        }
    }

    pub(crate) fn io(&mut self) -> (&mut T, &mut Connection) {
        (&mut self.transport, &mut self.conn)
    }

    /// The codec state (compression/encryption switches, metrics).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.transport.peer_addr()
    }

    pub fn latency(&self) -> Option<Duration> {
        self.transport.latency()
    }

    pub fn metrics(&self) -> ConnectionMetrics {
        self.conn.metrics()
    }

    pub async fn set_flush_mode(&mut self, mode: FlushMode) -> Result<(), JolyneError> {
        self.conn.set_flush_mode(mode, Instant::now())?;
        driver::after_send(&mut self.transport, &mut self.conn).await
    }

    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        self.conn.flush()?;
        driver::flush_out(&mut self.transport, &mut self.conn).await
    }

    pub async fn disconnect(
        mut self,
        reason: EnumsConnectionDisconnectFailReason,
        message: &str,
    ) -> Result<(), JolyneError> {
        self.conn
            .queue_disconnect(reason, message, Instant::now())?;
        driver::close(&mut self.transport, &mut self.conn).await
    }

    pub async fn close(mut self) -> Result<(), JolyneError> {
        driver::close(&mut self.transport, &mut self.conn).await
    }

    super::packet_io!();
}
