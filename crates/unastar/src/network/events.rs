//! Network events for communication between network tasks and main thread.

use glam::DVec3;
use jolyne::valentine::McpePacket;
use tokio::sync::mpsc;

use super::inbound::InboundPacket;

/// Unique session identifier.
pub type SessionId = u64;

/// Session lifecycle events from network tasks to the main thread.
///
/// Client packets do not travel here: each session has its own bounded inbound
/// queue (see [`super::inbound`]), handed over in [`NetworkEvent::Joined`].
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
        /// This session's inbound packet queue.
        inbound_rx: mpsc::Receiver<InboundPacket>,
    },

    /// Player disconnected (network task exiting).
    Disconnected { session_id: SessionId },
}

impl NetworkEvent {
    /// Get the session ID for any event type.
    pub fn session_id(&self) -> SessionId {
        match self {
            NetworkEvent::Joined { session_id, .. } => *session_id,
            NetworkEvent::Disconnected { session_id } => *session_id,
        }
    }
}
