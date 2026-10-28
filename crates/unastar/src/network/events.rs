//! Network events for communication between network tasks and main thread.

use glam::DVec3;
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
use tokio::sync::mpsc;

/// Unique session identifier.
pub type SessionId = u64;

/// Events from network tasks to the main thread.
///
/// All network communication is consolidated into this single enum,
/// reducing channel proliferation while maintaining parallel I/O.
#[derive(Debug)]
pub enum NetworkEvent {
    /// Player requested its chunk radius at the end of the join (streamed
    /// spawn: `PlayStatus(PlayerSpawn)` and the client's
    /// `SetLocalPlayerAsInitialized` are still to come).
    Joined {
        session_id: SessionId,
        display_name: String,
        xuid: Option<String>,
        uuid: Option<String>,
        runtime_id: i64,
        initial_position: DVec3,
        saved_data: Option<Box<crate::storage::PlayerData>>,
        /// The chunk radius granted to the client (`ChunkRadiusUpdated`).
        chunk_radius: i32,
        /// Bounded channel to send packets to this player.
        /// Uses bounded capacity to prevent memory explosion on slow connections.
        outbound_tx: mpsc::Sender<McpePacket>,
    },

    /// Player sent a packet.
    Packet {
        session_id: SessionId,
        packet_args: McpePacketArgs,
        packet: Box<BorrowedMcpePacket>,
    },

    /// Player disconnected (network task exiting).
    Disconnected { session_id: SessionId },
}

impl NetworkEvent {
    /// Get the session ID for any event type.
    pub fn session_id(&self) -> SessionId {
        match self {
            NetworkEvent::Joined { session_id, .. } => *session_id,
            NetworkEvent::Packet { session_id, .. } => *session_id,
            NetworkEvent::Disconnected { session_id } => *session_id,
        }
    }
}
