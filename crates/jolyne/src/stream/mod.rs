//! Async Bedrock connections over a [`Transport`]: Tier 2 of the API
//! (`docs/API.md`), the step-by-step join.
//!
//! [`BedrockStream<S, R, T>`] is a typestate: `S` is the join phase, `R` the
//! role (`Server` or `Client`), `T` the transport. Each phase wraps the
//! matching sans-IO owner from [`crate::proto`] and only exposes the methods
//! valid in that phase. Transitions consume the stream; a failed transition
//! returns [`Failed`], which still owns the connection so a reasoned
//! `Disconnect` can be sent.
//!
//! ```text
//! Server: Handshake --accept_network_settings--> Login --authenticate--> SecurePending
//!         --finish_handshake--> ResourcePacks --negotiate_packs[_with]--> StartGame
//!         --start_game--> Play
//! Client: Handshake --request_settings--> Login --send_login--> SecurePending
//!         --await_handshake--> ResourcePacks --handle_packs--> StartGame
//!         --await_start_game--> Play
//! ```
//!
//! Tier 1's `accept_join_sequence` and `join` run these chains back to back.
//! In [`Play`] the stream sends and receives packets (`recv_packet`,
//! `send_packet`, raw and borrowed variants, batches, flush modes);
//! [`BedrockStream::into_transport`] leaves the typestate for
//! [`BedrockTransport`]. `docs/ARCHITECTURE.md` describes the driver loop,
//! deadlines and player seats.

use std::marker::PhantomData;
use std::time::Duration;

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
pub(crate) mod slots;
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

mod sealed {
    /// Keeps the typestate traits closed to jolyne's own markers.
    pub trait Sealed {}
}

/// Marker for a join phase. Sealed.
pub trait State: sealed::Sealed {}

/// Maps a phase marker to the core owner it wraps for role `R`.
pub trait PhaseOf<R: Role>: State {
    /// The sans-IO owner from [`crate::proto`] driving this phase.
    type Owner: Owner + Send;
}

/// Marker for a connection role. Sealed.
pub trait Role: sealed::Sealed {}

/// Server role marker.
pub struct Server;
impl sealed::Sealed for Server {}
impl Role for Server {}

/// Client role marker.
pub struct Client;
impl sealed::Sealed for Client {}
impl Role for Client {}

macro_rules! states {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$doc])* pub struct $name;
            impl sealed::Sealed for $name {}
            impl State for $name {}
        )*
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
    // An `Option` only so consuming methods can move the parts out of a type
    // with `Drop` (safe Rust has no other way): `take(self)` consumes the
    // stream, so every `&self`/`&mut self` method sees `Some`, and only
    // `Drop` after a `take` sees `None`.
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

    #[expect(
        clippy::expect_used,
        reason = "consumes the stream: parts were set by from_parts and never taken before"
    )]
    pub(crate) fn take(mut self) -> Parts<T, S::Owner> {
        self.parts.take().expect("stream parts present")
    }

    #[expect(clippy::expect_used, reason = "only take(self) empties parts")]
    fn parts_ref(&self) -> &Parts<T, S::Owner> {
        self.parts.as_ref().expect("stream parts present")
    }

    #[expect(clippy::expect_used, reason = "only take(self) empties parts")]
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

    /// The transport's peer address (a placeholder for NetherNet).
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.parts_ref().transport.peer_addr()
    }

    /// Transport round-trip time, when the transport measures it.
    pub fn latency(&self) -> Option<Duration> {
        self.parts_ref().transport.latency()
    }

    /// A snapshot of the connection's packet, batch and byte counters.
    pub fn metrics(&self) -> ConnectionMetrics {
        self.parts_ref().owner.connection_ref().metrics()
    }

    /// The active flush mode.
    pub fn flush_mode(&self) -> FlushMode {
        self.parts_ref().owner.connection_ref().flush_mode()
    }

    /// Switches between immediate, manual and interval flushing.
    pub async fn set_flush_mode(&mut self, mode: FlushMode) -> Result<(), JolyneError> {
        let (transport, conn) = self.io();
        conn.set_flush_mode(mode, driver::now())?;
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
        conn.queue_disconnect(reason, message, driver::now())?;
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
            conn.queue_packet(&packet, driver::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a packet by reference.
        pub async fn send_packet_ref(&mut self, packet: &McpePacket) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_packet(packet, driver::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a raw packet (forwarded bytes, unknown IDs).
        pub async fn send_packet_raw(&mut self, packet: &RawPacket) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_raw(packet, driver::now())?;
            driver::after_send(transport, conn).await
        }

        /// Queues a custom packet (its ID must be outside valentine's range).
        pub async fn send_custom<P: CustomPacket>(
            &mut self,
            packet: &P,
        ) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.queue_custom(packet, driver::now())?;
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
            conn.send_batch(packets, reliable, driver::now())?;
            driver::after_send(transport, conn).await
        }

        /// Sends raw `packets` as one batch after anything already queued.
        pub async fn send_batch_raw(
            &mut self,
            packets: &[RawPacket],
            reliable: bool,
        ) -> Result<(), JolyneError> {
            let (transport, conn) = self.io();
            conn.send_batch_raw(packets, reliable, driver::now())?;
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
    //! Phase aliases over RakNet. The `Stream` suffix keeps them apart from
    //! the sans-IO owners in `proto` (`proto::server::ServerHandshake`, ...).
    use super::*;

    /// Server connection over RakNet; network settings not yet negotiated.
    pub type ServerHandshakeStream = BedrockStream<Handshake, Server, RakNetTransport>;
    /// Server connection over RakNet; login pending.
    pub type ServerLoginStream = BedrockStream<Login, Server, RakNetTransport>;
    /// Server connection over RakNet; encryption handshake pending.
    pub type ServerSecurePendingStream = BedrockStream<SecurePending, Server, RakNetTransport>;
    /// Server connection over RakNet; resource packs pending.
    pub type ServerResourcePacksStream = BedrockStream<ResourcePacks, Server, RakNetTransport>;
    /// Server connection over RakNet; `StartGame` pending.
    pub type ServerStartGameStream = BedrockStream<StartGame, Server, RakNetTransport>;
    /// Server connection over RakNet; spawned and exchanging game packets.
    pub type ServerPlayStream = BedrockStream<Play, Server, RakNetTransport>;

    /// Client connection over RakNet; network settings not yet negotiated.
    pub type ClientHandshakeStream = BedrockStream<Handshake, Client, RakNetTransport>;
    /// Client connection over RakNet; login pending.
    pub type ClientLoginStream = BedrockStream<Login, Client, RakNetTransport>;
    /// Client connection over RakNet; encryption handshake pending.
    pub type ClientSecurePendingStream = BedrockStream<SecurePending, Client, RakNetTransport>;
    /// Client connection over RakNet; resource packs pending.
    pub type ClientResourcePacksStream = BedrockStream<ResourcePacks, Client, RakNetTransport>;
    /// Client connection over RakNet; `StartGame` pending.
    pub type ClientStartGameStream = BedrockStream<StartGame, Client, RakNetTransport>;
    /// Client connection over RakNet; spawned and exchanging game packets.
    pub type ClientPlayStream = BedrockStream<Play, Client, RakNetTransport>;
}

#[cfg(feature = "nethernet")]
pub mod nethernet_types {
    //! Phase aliases over NetherNet (see [`super::raknet_types`]).
    use super::*;

    /// Server connection over NetherNet; network settings not yet negotiated.
    pub type ServerHandshakeStream = BedrockStream<Handshake, Server, NetherNetTransport>;
    /// Server connection over NetherNet; login pending.
    pub type ServerLoginStream = BedrockStream<Login, Server, NetherNetTransport>;
    /// Server connection over NetherNet; encryption handshake pending.
    pub type ServerSecurePendingStream = BedrockStream<SecurePending, Server, NetherNetTransport>;
    /// Server connection over NetherNet; resource packs pending.
    pub type ServerResourcePacksStream = BedrockStream<ResourcePacks, Server, NetherNetTransport>;
    /// Server connection over NetherNet; `StartGame` pending.
    pub type ServerStartGameStream = BedrockStream<StartGame, Server, NetherNetTransport>;
    /// Server connection over NetherNet; spawned and exchanging game packets.
    pub type ServerPlayStream = BedrockStream<Play, Server, NetherNetTransport>;

    /// Client connection over NetherNet; network settings not yet negotiated.
    pub type ClientHandshakeStream = BedrockStream<Handshake, Client, NetherNetTransport>;
    /// Client connection over NetherNet; login pending.
    pub type ClientLoginStream = BedrockStream<Login, Client, NetherNetTransport>;
    /// Client connection over NetherNet; encryption handshake pending.
    pub type ClientSecurePendingStream = BedrockStream<SecurePending, Client, NetherNetTransport>;
    /// Client connection over NetherNet; resource packs pending.
    pub type ClientResourcePacksStream = BedrockStream<ResourcePacks, Client, NetherNetTransport>;
    /// Client connection over NetherNet; `StartGame` pending.
    pub type ClientStartGameStream = BedrockStream<StartGame, Client, NetherNetTransport>;
    /// Client connection over NetherNet; spawned and exchanging game packets.
    pub type ClientPlayStream = BedrockStream<Play, Client, NetherNetTransport>;
}

#[cfg(all(test, feature = "server", feature = "client"))]
mod tests;
