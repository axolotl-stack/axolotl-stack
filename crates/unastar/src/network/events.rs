//! Network events for communication between network tasks and main thread.

use glam::DVec3;
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
use tokio::sync::mpsc;

/// Unique session identifier.
pub type SessionId = u64;

/// Events from network tasks to the main thread.
///
/// Connection control and packet traffic cross this bounded queue. Metrics attach
/// a read handle once, so observation updates add no per-tick queue traffic.
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

    /// Attach the read handle produced by this exact network task after join.
    MetricsReady {
        session_id: SessionId,
        metrics: crate::entity::components::PlayerNetworkMetrics,
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
            NetworkEvent::MetricsReady { session_id, .. } => *session_id,
        }
    }
}
