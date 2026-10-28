//! The play-state owner shared by both roles.
//!
//! A [`Session`] is what a completed join leaves behind: the [`Connection`]
//! (with compression and encryption as negotiated, and any packets deferred
//! during the join already replayed into its inbound queue) plus the runtime
//! ID and chunk radius agreed while spawning. `ServerSession` and
//! `ClientSession` wrap it with role-specific data.
//!
//! A server session handed over by a streamed spawn
//! (`WorldReady::start_game_streamed`) starts before the client has
//! initialized: until `SetLocalPlayerAsInitialized` arrives, [`Session::next_packet`]
//! checks its runtime ID and enforces the spawn deadline.

use std::time::Instant;

use super::conn::Connection;
use super::phase::Owner;
use super::raw::RawPacket;
use crate::error::{HandshakePhase, JolyneError, ProtocolError};
use crate::valentine::{McpePacketData, McpePacketName};

/// A spawned connection: the codec plus what the join sequence learned.
pub struct Session {
    conn: Connection,
    runtime_id: u64,
    chunk_radius: Option<i32>,
    /// The spawn deadline while `SetLocalPlayerAsInitialized` is pending.
    spawn_deadline: Option<Instant>,
}

impl Session {
    /// Wraps a connection that finished the join sequence with the player's runtime
    /// entity ID and the agreed chunk radius.
    pub fn new(conn: Connection, runtime_id: u64, chunk_radius: Option<i32>) -> Self {
        Self {
            conn,
            runtime_id,
            chunk_radius,
            spawn_deadline: None,
        }
    }

    /// A server session whose client has not sent `SetLocalPlayerAsInitialized`
    /// yet; it must by `spawn_deadline`.
    #[cfg(feature = "server")]
    pub(crate) fn spawning(
        conn: Connection,
        runtime_id: u64,
        chunk_radius: i32,
        spawn_deadline: Instant,
    ) -> Self {
        Self {
            spawn_deadline: Some(spawn_deadline),
            ..Self::new(conn, runtime_id, Some(chunk_radius))
        }
    }

    /// The player's runtime entity ID from `StartGame`.
    pub fn runtime_id(&self) -> u64 {
        self.runtime_id
    }

    /// The chunk radius agreed during spawn (`None` if the client initialized
    /// before requesting one).
    pub fn chunk_radius(&self) -> Option<i32> {
        self.chunk_radius
    }

    /// Whether the client has sent `SetLocalPlayerAsInitialized`. Only a
    /// streamed spawn hands over a session that has not.
    pub fn is_initialized(&self) -> bool {
        self.spawn_deadline.is_none()
    }

    /// When an uninitialized session fails with `Timeout(Spawn)`.
    pub fn spawn_deadline(&self) -> Option<Instant> {
        self.spawn_deadline
    }

    /// The next inbound packet. While the client has not initialized, a
    /// `SetLocalPlayerAsInitialized` must carry the `StartGame` runtime ID
    /// (it is still returned), and `now` past the spawn deadline is
    /// `Timeout(Spawn)`.
    pub fn next_packet(&mut self, now: Instant) -> Result<Option<RawPacket>, JolyneError> {
        let packet = self.conn.next_packet()?;
        match &packet {
            Some(packet) => self.check_initialized(packet)?,
            None if self.spawn_deadline.is_some_and(|deadline| now >= deadline) => {
                return Err(ProtocolError::Timeout(HandshakePhase::Spawn).into());
            }
            None => {}
        }
        Ok(packet)
    }

    /// Ends the pending spawn on a `SetLocalPlayerAsInitialized` with the
    /// player's runtime ID.
    pub(crate) fn check_initialized(&mut self, packet: &RawPacket) -> Result<(), JolyneError> {
        if self.spawn_deadline.is_none()
            || !packet.is(McpePacketName::SetLocalPlayerAsInitializedPacket)
        {
            return Ok(());
        }
        let McpePacketData::SetLocalPlayerAsInitializedPacket(initialized) =
            packet.clone().decode()?.data
        else {
            return Ok(()); // the header named it; decoding cannot yield another
        };
        let got = initialized.player_id.actor_runtime_id;
        if got != self.runtime_id {
            return Err(ProtocolError::RuntimeIdMismatch {
                expected: self.runtime_id,
                got,
            }
            .into());
        }
        self.spawn_deadline = None;
        tracing::debug!(runtime_id = got, "player initialized");
        Ok(())
    }
}

impl Owner for Session {
    fn connection(&mut self) -> &mut Connection {
        &mut self.conn
    }
    fn connection_ref(&self) -> &Connection {
        &self.conn
    }
    fn into_connection(self) -> Connection {
        self.conn
    }
    fn play_session(&mut self) -> Option<&mut Session> {
        Some(self)
    }
}
