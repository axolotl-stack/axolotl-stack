//! Player-specific components.

use bevy_ecs::prelude::*;
use jolyne::protocol::types::mcpe::McpePacket;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::transform::Position;

/// Marker for player entities.
#[derive(Component, Debug)]
pub struct Player;

/// Player display name for network sync.
#[derive(Component, Debug, Clone)]
pub struct PlayerName(pub String);

/// Player UUID for network sync.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlayerUuid(pub Uuid);

/// Network session data for a player.
#[derive(Component)]
pub struct PlayerSession {
    pub session_id: u64,
    pub display_name: String,
    pub xuid: Option<String>,
    pub uuid: Option<String>,
    pub outbound_tx: mpsc::UnboundedSender<McpePacket>,
}

impl std::fmt::Debug for PlayerSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerSession")
            .field("session_id", &self.session_id)
            .field("display_name", &self.display_name)
            .field("xuid", &self.xuid)
            .field("uuid", &self.uuid)
            .finish()
    }
}

impl PlayerSession {
    /// Send a packet to this player.
    pub fn send(&self, packet: McpePacket) -> Result<(), mpsc::error::SendError<McpePacket>> {
        self.outbound_tx.send(packet)
    }
}

/// Player game mode.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub fn allows_damage(&self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Adventure)
    }

    pub fn allows_flight(&self) -> bool {
        matches!(self, GameMode::Creative | GameMode::Spectator)
    }

    pub fn has_collision(&self) -> bool {
        !matches!(self, GameMode::Spectator)
    }

    pub fn can_break_blocks(&self) -> bool {
        matches!(self, GameMode::Survival | GameMode::Creative)
    }

    pub fn instant_break(&self) -> bool {
        matches!(self, GameMode::Creative)
    }
}

/// Player movement state flags.
#[derive(Component, Debug, Default)]
pub struct PlayerState {
    pub sneaking: bool,
    pub sprinting: bool,
    pub swimming: bool,
    pub gliding: bool,
    pub flying: bool,
}

/// Per-tick player input from PlayerAuthInput packet.
/// Updated each tick from client input.
#[derive(Component, Debug, Default)]
pub struct PlayerInput {
    /// Movement vector (WASD/controller stick)
    pub move_x: f32,
    pub move_z: f32,
    /// Jump is pressed this tick
    pub jumping: bool,
    /// On ground flag (client reported)
    pub on_ground: bool,
    /// Sneaking state
    pub sneaking: bool,
    /// Sprinting state
    pub sprinting: bool,
    /// Client tick this input was sent
    pub tick: i64,
}

/// Chunk view radius for this player.
#[derive(Component, Debug, Clone, Copy)]
pub struct ChunkRadius(pub i32);

impl Default for ChunkRadius {
    fn default() -> Self {
        Self(8)
    }
}

/// Runtime entity ID for network synchronization.
#[derive(Component, Debug, Clone, Copy)]
pub struct RuntimeEntityId(pub i64);

impl Default for RuntimeEntityId {
    fn default() -> Self {
        Self(1)
    }
}

/// Player food/hunger state.
#[derive(Component, Debug, Clone)]
pub struct Hunger {
    pub food_level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
}

impl Default for Hunger {
    fn default() -> Self {
        Self {
            food_level: 20,
            saturation: 5.0,
            exhaustion: 0.0,
        }
    }
}

impl Hunger {
    pub fn exhaust(&mut self, amount: f32) {
        self.exhaustion += amount;
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else if self.food_level > 0 {
                self.food_level -= 1;
            }
        }
    }

    pub fn can_sprint(&self) -> bool {
        self.food_level > 6
    }

    pub fn can_regenerate(&self) -> bool {
        self.food_level >= 18
    }
}

/// Player experience.
#[derive(Component, Debug, Default, Clone)]
pub struct Experience {
    pub level: i32,
    pub progress: f32, // 0.0 to 1.0
}

/// Block breaking state for survival mode.
/// Tracks ongoing block breaking for crack animation and anti-cheat validation.
#[derive(Component, Debug, Default)]
pub struct BreakingState {
    /// Position of block being broken (world coords)
    pub position: Option<(i32, i32, i32)>,
    /// Server tick when breaking started
    pub start_tick: u64,
    /// Expected break time in ticks (20 ticks/sec)
    /// Default hardness = 1.0s = 20 ticks for bare hand
    pub expected_ticks: u32,
    /// Break counter for timing sounds/effects (incremented each tick)
    /// Every 5 ticks, play breaking sound/particles
    break_counter: u32,
}

impl BreakingState {
    /// Start breaking a block at the given position.
    pub fn start(&mut self, x: i32, y: i32, z: i32, current_tick: u64, break_time_ticks: u32) {
        self.position = Some((x, y, z));
        self.start_tick = current_tick;
        self.expected_ticks = break_time_ticks;
        self.break_counter = 0;
    }

    /// Stop breaking (abort or finish).
    pub fn stop(&mut self) {
        self.position = None;
        self.break_counter = 0;
    }

    /// Check if currently breaking a block.
    pub fn is_breaking(&self) -> bool {
        self.position.is_some()
    }

    /// Tick the breaking state. Call this once per server tick while breaking.
    /// Returns `true` every 5 ticks (when sound/particles should be played).
    pub fn tick(&mut self) -> bool {
        if self.position.is_none() {
            return false;
        }
        self.break_counter += 1;
        self.break_counter % 5 == 0
    }

    /// Get the current break counter.
    pub fn break_counter(&self) -> u32 {
        self.break_counter
    }

    /// Validate if enough time has passed to break the block.
    /// Returns true if elapsed >= expected (with 10% tolerance for network latency).
    pub fn validate_break(&self, current_tick: u64) -> bool {
        if self.position.is_none() {
            return false;
        }
        let elapsed = current_tick.saturating_sub(self.start_tick) as u32;
        // Allow 10% tolerance for network latency
        let min_required = (self.expected_ticks as f32 * 0.9) as u32;
        elapsed >= min_required
    }
}

// =============================================================================
// Broadcast-related components
// =============================================================================

/// Component marking a player as newly spawned this tick.
/// Removed after broadcasting to other players.
#[derive(Component, Debug)]
pub struct PendingSpawnBroadcast;

/// Component marking a player for despawn broadcast.
/// Added when player is about to be removed.
#[derive(Component, Debug)]
pub struct PendingDespawnBroadcast;

/// Component tracking the last broadcast position.
/// Used to detect significant movement for broadcasting.
#[derive(Component, Debug, Default)]
pub struct LastBroadcastPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

/// Component tracking the player's current chunk for spatial hashing.
/// Updated when player crosses chunk boundaries.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct SpatialChunk {
    pub x: i32,
    pub z: i32,
}

impl SpatialChunk {
    /// Create from world position.
    /// Uses floor() to correctly handle negative coordinates.
    pub fn from_position(pos: &Position) -> Self {
        Self {
            x: (pos.0.x.floor() as i32) >> 4,
            z: (pos.0.z.floor() as i32) >> 4,
        }
    }

    /// Get as tuple key.
    pub fn as_tuple(&self) -> (i32, i32) {
        (self.x, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn test_spatial_chunk_positive_coords() {
        let pos = Position(DVec3::new(0.5, 64.0, 0.5));
        let chunk = SpatialChunk::from_position(&pos);
        assert_eq!(chunk.x, 0);
        assert_eq!(chunk.z, 0);

        let pos = Position(DVec3::new(16.0, 64.0, 32.0));
        let chunk = SpatialChunk::from_position(&pos);
        assert_eq!(chunk.x, 1);
        assert_eq!(chunk.z, 2);
    }

    #[test]
    fn test_spatial_chunk_negative_coords() {
        // x = -0.5 should map to chunk -1, not 0
        let pos = Position(DVec3::new(-0.5, 64.0, 0.5));
        let chunk = SpatialChunk::from_position(&pos);
        assert_eq!(chunk.x, -1, "x=-0.5 should be chunk -1");
        assert_eq!(chunk.z, 0);

        // x = -16.0 should map to chunk -1 (boundary of chunk -1)
        let pos = Position(DVec3::new(-16.0, 64.0, -16.0));
        let chunk = SpatialChunk::from_position(&pos);
        assert_eq!(chunk.x, -1);
        assert_eq!(chunk.z, -1);

        // x = -16.1 should map to chunk -2
        let pos = Position(DVec3::new(-16.1, 64.0, 0.0));
        let chunk = SpatialChunk::from_position(&pos);
        assert_eq!(chunk.x, -2);
    }
}
