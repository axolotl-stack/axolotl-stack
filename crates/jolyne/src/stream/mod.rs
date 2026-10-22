//! Async Bedrock connections over a [`Transport`].
//!
//! [`BedrockStream<S, R, T>`] is a typestate: `S` is the join phase, `R` the
//! role (`Server` or `Client`), `T` the transport. Each phase wraps the
//! matching sans-IO owner from [`crate::proto`] and only exposes the methods
//! valid in that phase. Transitions consume the stream; a failed transition
//! returns [`Failed`], which still owns the connection so a reasoned
//! `Disconnect` can be sent.

use std::marker::PhantomData;
use std::time::{Duration, Instant};

use crate::config::FlushMode;
use crate::error::JolyneError;
use crate::proto::conn::Connection;
use crate::proto::custom::CustomPacket;
use crate::proto::observe::ConnectionMetrics;
use crate::proto::phase::Owner;
use crate::proto::raw::RawPacket;
use crate::valentine::{BorrowedMcpePacket, EnumsConnectionDisconnectFailReason, McpePacket};

pub(crate) mod driver;
mod failed;
mod link;
pub mod slots;
pub mod transport;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

pub use failed::Failed;
pub use link::BedrockTransport;
use slots::Seat;
#[cfg(feature = "nethernet")]
pub use transport::NetherNetTransport;
#[cfg(feature = "raknet")]
pub use transport::RakNetTransport;
pub use transport::Transport;

/// Marker for a join phase.
pub trait State {}

/// Maps a phase marker to the core owner it wraps for role `R`.
pub trait PhaseOf<R: Role>: State {
    type Owner: Owner + Send;
}

/// Marker for a connection role.
pub trait Role {}

/// Server role marker.
pub struct Server;
impl Role for Server {}

/// Client role marker.
pub struct Client;
impl Role for Client {}

macro_rules! states {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $($(#[$doc])* pub struct $name; impl State for $name {})*
    };
}

states! {
    /// Connected; network settings not yet negotiated.
    Handshake,
    /// Compression negotiated; login pending.
    Login,
    /// Login verified; encryption handshake pending.
    SecurePending,
    /// Logged in; resource packs pending.
    ResourcePacks,
    /// Packs done; `StartGame` pending.
    StartGame,
    /// Spawned; exchanging game packets.
    Play,
}

pub(crate) struct Parts<T, O> {
    pub transport: T,
    pub owner: O,
    pub seat: Seat,
}

/// A Bedrock connection in join phase `S`.
pub struct BedrockStream<S: PhaseOf<R>, R: Role, T: Transport> {
    // `None` only while a consuming method moves the parts out.
    parts: Option<Parts<T, S::Owner>>,
    _marker: PhantomData<fn() -> (S, R)>,
}

impl<S: PhaseOf<R>, R: Role, T: Transport> Drop for BedrockStream<S, R, T> {
    /// Sends whatever the transport accepts without waiting, so packets
    /// queued in a buffering flush mode are not silently lost.
    fn drop(&mut self) {
        if let Some(parts) = self.parts.as_mut() {
            driver::flush_now(&mut parts.transport, parts.owner.connection());
        }
    }
}

impl<S: PhaseOf<R>, R: Role, T: Transport> BedrockStream<S, R, T> {
    #[cfg(any(feature = "server", feature = "client"))]
    pub(crate) fn from_parts(transport: T, owner: S::Owner, seat: Seat) -> Self {
        Self {
            parts: Some(Parts {
                transport,
                owner,
                seat,
            }),
            _marker: PhantomData,
        }
    }

    pub(crate) fn take(mut self) -> Parts<T, S::Owner> {
        self.parts.take().expect("stream parts present")
    }

    fn parts_ref(&self) -> &Parts<T, S::Owner> {
        self.parts.as_ref().expect("stream parts present")
    }

    pub(crate) fn parts_mut(&mut self) -> &mut Parts<T, S::Owner> {
        self.parts.as_mut().expect("stream parts present")
    }

    pub(crate) fn io(&mut self) -> (&mut T, &mut Connection) {
        let parts = self.parts_mut();
        (&mut parts.transport, parts.owner.connection())
    }

    /// The core owner of the current phase.
    pub fn owner(&self) -> &S::Owner {
        &self.parts_ref().owner
    }

    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.parts_ref().transport.peer_addr()
    }

    /// Transport round-trip time, when the transport measures it.
    pub fn latency(&self) -> Option<Duration> {
        self.parts_ref().transport.latency()
    }

    pub fn metrics(&self) -> ConnectionMetrics {
        self.parts_ref().owner.connection_ref().metrics()
    }

    pub fn flush_mode(&self) -> FlushMode {
        self.parts_ref().owner.connection_ref().flush_mode()
    }

    /// Switches between immediate, manual and interval flushing.
    pub async fn set_flush_mode(&mut self, mode: FlushMode) -> Result<(), JolyneError> {
        let (transport, conn) = self.io();
        conn.set_flush_mode(mode, Instant::now())?;
        driver::after_send(transport, conn).await
    }

    /// Seals queued packets into one batch and writes it.
    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        let (transport, conn) = self.io();
        conn.flush()?;
        driver::flush_out(transport, conn).await
    }

    /// Sends `Disconnect{reason, message}`, drains pending writes and closes.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn disconnect(
        mut self,
        reason: EnumsConnectionDisconnectFailReason,
        message: &str,
    ) -> Result<(), JolyneError> {
        // Through `io()`: if this fails or is cancelled, `Drop` still sends
        // what was queued.
        let (transport, conn) = self.io();
        conn.queue_disconnect(reason, message, Instant::now())?;
        driver::close(transport, conn).await
    }

    /// Drains pending writes and closes the transport.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn close(mut self) -> Result<(), JolyneError> {
        let (transport, conn) = self.io();
        driver::close(transport, conn).await
    }

    /// Leaves the typestate for raw packet I/O (proxies, custom flows).
    pub fn into_transport(self) -> BedrockTransport<T> {
        let parts = self.take();
        BedrockTransport::from_parts(parts.transport, parts.owner.into_connection(), parts.seat)
    }
}

/// Play-state packet I/O, shared by [`BedrockStream<Play, _, _>`] and
/// [`BedrockTransport`].
macro_rules! packet_io {
    () => {
        /// The next packet with only its header parsed.
        pub async fn recv_packet_raw(&mut self) -> Result<RawPacket, JolyneError> {
            let (transport, conn) = self.io();
            driver::recv_raw(transport, conn).await
        }

        /// The next packet as a borrowed view (no payload allocation).
        pub async fn recv_packet_borrowed(&mut self) -> Result<BorrowedMcpePacket, JolyneError> {
            let packet = self.recv_packet_raw().await?;
            let result = packet.decode_borrowed();
            if result
                .as_ref()
                .is_err_and(|error| !matches!(error, JolyneError::NonValentinePacket(_)))
            {
                self.io().1.record_decode_error();
            }
            result
        }

        /// The next packet as an owned value.
        pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
            let packet = self.recv_packet_raw().await?;
            let result = packet.decode();
            if result
                .as_ref()
                .is_err_and(|error| !matches!(error, JolyneError::NonValentinePacket(_)))
            {
                self.io().1.record_decode_error();
            }
            result
        }

        /// Queues a packet; writes immediately unless a buffering flush mode
        /// is active (or the pending batch hit its limits).
        pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_packet(&packet, Instant::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a packet by reference.
        pub async fn send_packet_ref(&mut self, packet: &McpePacket) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_packet(packet, Instant::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a raw packet (forwarded bytes, unknown IDs).
        pub async fn send_packet_raw(&mut self, packet: &RawPacket) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_raw(packet, Instant::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a custom packet (its ID must be outside valentine's range).
        pub async fn send_custom<P: CustomPacket>(
            &mut self,
            packet: &P,
        ) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_custom(packet, Instant::now())?;
            driver::after_send(transport, conn).await
        }

        /// Sends `packets` as one batch after anything already queued.
        /// Unreliable delivery is refused while encrypted.
        pub async fn send_batch(
            &mut self,
            packets: &[McpePacket],
            reliable: bool,
        ) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.send_batch(packets, reliable, Instant::now())?;
            driver::after_send(transport, conn).await
        }

        /// Sends raw `packets` as one batch after anything already queued.
        pub async fn send_batch_raw(
            &mut self,
            packets: &[RawPacket],
            reliable: bool,
        ) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.send_batch_raw(packets, reliable, Instant::now())?;
            driver::after_send(transport, conn).await
        }
    };
}
pub(crate) use packet_io;

impl<R: Role, T: Transport> BedrockStream<Play, R, T>
where
    Play: PhaseOf<R>,
{
    packet_io!();
}

#[cfg(feature = "raknet")]
pub mod raknet_types {
    //! Phase aliases over RakNet.
    use super::*;

    pub type ServerHandshake = BedrockStream<Handshake, Server, RakNetTransport>;
    pub type ServerLogin = BedrockStream<Login, Server, RakNetTransport>;
    pub type ServerSecurePending = BedrockStream<SecurePending, Server, RakNetTransport>;
    pub type ServerResourcePacks = BedrockStream<ResourcePacks, Server, RakNetTransport>;
    pub type ServerStartGame = BedrockStream<StartGame, Server, RakNetTransport>;
    pub type ServerPlay = BedrockStream<Play, Server, RakNetTransport>;

    pub type ClientHandshake = BedrockStream<Handshake, Client, RakNetTransport>;
    pub type ClientLogin = BedrockStream<Login, Client, RakNetTransport>;
    pub type ClientSecurePending = BedrockStream<SecurePending, Client, RakNetTransport>;
    pub type ClientResourcePacks = BedrockStream<ResourcePacks, Client, RakNetTransport>;
    pub type ClientStartGame = BedrockStream<StartGame, Client, RakNetTransport>;
    pub type ClientPlay = BedrockStream<Play, Client, RakNetTransport>;
}

#[cfg(feature = "nethernet")]
pub mod nethernet_types {
    //! Phase aliases over NetherNet.
    use super::*;

    pub type ServerHandshake = BedrockStream<Handshake, Server, NetherNetTransport>;
    pub type ServerLogin = BedrockStream<Login, Server, NetherNetTransport>;
    pub type ServerSecurePending = BedrockStream<SecurePending, Server, NetherNetTransport>;
    pub type ServerResourcePacks = BedrockStream<ResourcePacks, Server, NetherNetTransport>;
    pub type ServerStartGame = BedrockStream<StartGame, Server, NetherNetTransport>;
    pub type ServerPlay = BedrockStream<Play, Server, NetherNetTransport>;

    pub type ClientHandshake = BedrockStream<Handshake, Client, NetherNetTransport>;
    pub type ClientLogin = BedrockStream<Login, Client, NetherNetTransport>;
    pub type ClientSecurePending = BedrockStream<SecurePending, Client, NetherNetTransport>;
    pub type ClientResourcePacks = BedrockStream<ResourcePacks, Client, NetherNetTransport>;
    pub type ClientStartGame = BedrockStream<StartGame, Client, NetherNetTransport>;
    pub type ClientPlay = BedrockStream<Play, Client, NetherNetTransport>;
}

#[cfg(all(test, feature = "server", feature = "client"))]
mod tests;
