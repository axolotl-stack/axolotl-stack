//! The play-state owner shared by both roles.
//!
//! A [`Session`] is what a completed join leaves behind: the [`Connection`]
//! (with compression and encryption as negotiated, and any packets deferred
//! during the join already replayed into its inbound queue) plus the runtime
//! ID and chunk radius agreed while spawning. `ServerSession` and
//! `ClientSession` wrap it with role-specific data.

use super::conn::Connection;
use super::phase::Owner;

/// A spawned connection: the codec plus what the join sequence learned.
pub struct Session {
    conn: Connection,
    runtime_id: u64,
    chunk_radius: Option<i32>,
}

impl Session {
    /// Wraps a connection that finished the join sequence with the player's runtime
    /// entity ID and the agreed chunk radius.
    pub fn new(conn: Connection, runtime_id: u64, chunk_radius: Option<i32>) -> Self {
        Self {
            conn,
            runtime_id,
            chunk_radius,
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
}
