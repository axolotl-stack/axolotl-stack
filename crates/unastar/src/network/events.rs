//! Network events for communication between network tasks and main thread.

use glam::DVec3;
use jolyne::protocol::types::mcpe::McpePacket;
use tokio::sync::mpsc;

/// Unique session identifier.
pub type SessionId = u64;

/// Events from network tasks to the main thread.
///
/// All network communication is consolidated into this single enum,
/// reducing channel proliferation while maintaining parallel I/O.
#[derive(Debug)]
pub enum NetworkEvent {
    /// Player completed handshake and joined the server.
    Joined {
        session_id: SessionId,
        display_name: String,
        xuid: Option<String>,
        uuid: Option<String>,
        runtime_id: i64,
        initial_position: DVec3,
        /// Channel to send packets to this player.
        outbound_tx: mpsc::UnboundedSender<McpePacket>,
    },

    /// Player sent a packet.
    Packet {
        session_id: SessionId,
        packet: McpePacket,
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
