//! The play-state owner shared by both roles.

use super::conn::Connection;
use super::phase::Owner;

/// A spawned connection: the codec plus what the join sequence learned.
pub struct Session {
    conn: Connection,
    runtime_id: u64,
    chunk_radius: Option<i32>,
}

impl Session {
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
