//! Player-specific components.

use std::sync::atomic::{AtomicU32, Ordering};

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use jolyne::valentine::McpePacket;
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
    /// Bounded outbound channel to prevent memory explosion on slow connections.
    pub outbound_tx: mpsc::Sender<McpePacket>,
    /// Count of dropped packets due to channel being full.
    /// Uses atomic for interior mutability, allowing `send()` to take `&self`.
    packets_dropped: AtomicU32,
}

impl std::fmt::Debug for PlayerSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerSession")
            .field("session_id", &self.session_id)
            .field("display_name", &self.display_name)
            .field("xuid", &self.xuid)
            .field("uuid", &self.uuid)
            .field(
                "packets_dropped",
                &self.packets_dropped.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl PlayerSession {
    /// Create a new PlayerSession.
    pub fn new(
        session_id: u64,
        display_name: String,
        xuid: Option<String>,
        uuid: Option<String>,
        outbound_tx: mpsc::Sender<McpePacket>,
    ) -> Self {
        Self {
            session_id,
            display_name,
            xuid,
            uuid,
            outbound_tx,
            packets_dropped: AtomicU32::new(0),
        }
    }

    /// Send a packet to this player.
    ///
    /// Uses `try_send` to avoid blocking the game thread.
    /// If the channel is full (client connection is slow), the packet is dropped
    /// and the client will request it again. This prevents memory explosion
    /// from accumulating packets for slow connections.
    ///
    /// Returns `true` if the packet was sent, `false` if dropped.
    pub fn send(&self, packet: McpePacket) -> bool {
        match self.outbound_tx.try_send(packet) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                let dropped = self.packets_dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped == 1 || dropped.is_multiple_of(100) {
                    tracing::warn!(
                        session_id = %self.session_id,
                        packets_dropped = dropped,
                        "outbound channel full, dropping packet - client connection may be slow"
                    );
                }
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Channel closed, player is disconnecting
                false
            }
        }
    }

    /// Get the number of dropped packets.
    pub fn packets_dropped(&self) -> u32 {
        self.packets_dropped.load(Ordering::Relaxed)
    }
}

/// Player game mode. In `unastar.toml` (`server.default_gamemode`) it is one of
/// `"survival"`, `"creative"`, `"adventure"` or `"spectator"`; anything else fails to load.
#[derive(
    Component, Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
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

impl PlayerInput {
    /// The `PlayerInputTick` a packet about this player carries: the tick of
    /// the player's latest `PlayerAuthInput` (0 before the first one).
    ///
    /// Clients treat this as their own input timeline and reject a value
    /// that goes backwards, so it must never be mixed with the server tick.
    pub fn wire_tick(&self) -> jolyne::valentine::types::PlayerInputTick {
        jolyne::valentine::types::PlayerInputTick {
            inputtick: u64::try_from(self.tick).unwrap_or(0),
        }
    }
}

/// The `PlayerInputTick` a packet about `entity` carries: the player's latest
/// input tick ([`PlayerInput::wire_tick`]), or 0 for any other actor.
pub fn input_tick_of(world: &World, entity: Entity) -> jolyne::valentine::types::PlayerInputTick {
    world.get::<PlayerInput>(entity).map_or(
        jolyne::valentine::types::PlayerInputTick { inputtick: 0 },
        PlayerInput::wire_tick,
    )
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
    pub const MAX_FOOD: i32 = 20;
    /// Exhaustion that costs one saturation (or food) point.
    pub const EXHAUSTION_PER_POINT: f32 = 4.0;
    /// Upper end of the exhaustion attribute (BDS join capture).
    pub const MAX_EXHAUSTION: f32 = 20.0;

    /// Add exhaustion. It is turned into lost saturation or food by the food
    /// tick, one point per tick (`server::game::hunger`).
    pub fn exhaust(&mut self, amount: f32) {
        if amount > 0.0 {
            self.exhaustion = (self.exhaustion + amount).min(Self::MAX_EXHAUSTION);
        }
    }

    /// Eat: `nutrition` food points and `nutrition × modifier × 2`
    /// saturation, saturation capped at the new food level.
    pub fn eat(&mut self, nutrition: i32, saturation_modifier: f32) {
        self.food_level = (self.food_level + nutrition).clamp(0, Self::MAX_FOOD);
        let gained = nutrition as f32 * saturation_modifier * 2.0;
        self.saturation = (self.saturation + gained).clamp(0.0, self.food_level as f32);
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

/// A survival block break in progress. Like BDS, the break advances once per
/// `PlayerAuthInput` of the breaking player, not per server tick (BDS
/// 1.26.51, scripted client: a client that sent no input for 10 ticks paused
/// its break; a 15-tick dirt break completed on the 15th input). Only
/// `AbortDestroyBlock` or a break elsewhere ends it: a completed break goes
/// on at that position from zero.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct BreakingState {
    /// Position being broken (world coords).
    pub position: Option<(i32, i32, i32)>,
    /// The block there when the break started (air included).
    pub block: u32,
    /// Face the break works on (its crack particles show on that side).
    pub face: i32,
    /// Fraction of the block destroyed so far.
    pub progress: f64,
    /// Fraction one input adds: the block, held item and player state when
    /// the break started or last continued.
    pub speed: f64,
    /// A break the server completed itself, waiting for plugin approval:
    /// its commit also corrects the breaking client, which never predicted it.
    pub completing: Option<(i32, i32, i32)>,
    /// Inputs since the break started or moved to another position.
    pub ticks: u32,
    /// Whether the other players still hear the break's hit sound: from a
    /// start on a block that can break until an input finds none there.
    pub sounding: bool,
}

impl BreakingState {
    /// Progress closer to 1 than this counts as complete: fifteen inputs of
    /// 1/15 add up to 0.9999999999999998.
    const COMPLETE: f64 = 1.0 - 1e-6;

    /// `StartDestroyBlock` on `block` at `position`, `speed` per input (0
    /// when the block there cannot break). It begins again from zero unless
    /// that block is the one being broken there: dirt put back after a break
    /// kept the progress, air did not (BDS).
    pub fn start(&mut self, position: (i32, i32, i32), block: u32, face: i32, speed: f64) {
        if self.position == Some(position) && self.block != block {
            self.progress = 0.0;
        }
        self.block = block;
        self.ticks = 0;
        self.sounding = speed > 0.0;
        self.continue_at(position, block, face, speed);
    }

    /// `ContinueDestroyBlock` on `block` at `position`: another position
    /// begins again from zero. Where no block can break, the speed of the
    /// last one is kept, so the break goes on over the air (BDS).
    pub fn continue_at(&mut self, position: (i32, i32, i32), block: u32, face: i32, speed: f64) {
        if self.position != Some(position) {
            self.progress = 0.0;
            self.block = block;
            self.speed = speed;
            self.ticks = 0;
            self.sounding = speed > 0.0;
        } else if speed > 0.0 {
            self.speed = speed;
        }
        self.position = Some(position);
        self.face = face;
    }

    /// Stop breaking (`AbortDestroyBlock`).
    pub fn stop(&mut self) {
        self.position = None;
        self.progress = 0.0;
        self.speed = 0.0;
        self.sounding = false;
    }

    /// Whether a block is being broken.
    pub fn is_breaking(&self) -> bool {
        self.position.is_some()
    }

    /// Add one input's progress.
    pub fn advance(&mut self) {
        self.progress += self.speed;
    }

    /// Whether the block is destroyed.
    pub fn is_complete(&self) -> bool {
        self.progress >= Self::COMPLETE
    }
}

// =============================================================================
// Broadcast-related components
// =============================================================================

// NOTE: PendingSpawnBroadcast and PendingDespawnBroadcast have been removed.
// They have been replaced by PlayerSpawnedEvent and PlayerDespawnedEvent (in world::ecs::events)
// to eliminate archetype changes during player spawn/despawn.

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
///
/// Uses component hooks to automatically synchronize with EntityGrid:
/// - `on_insert`: Adds entity to the grid at its chunk position
/// - `on_remove`: Removes entity from the grid
///
/// Note: Chunk boundary crossing during movement is handled by `sync_spatial_chunks` system,
/// which updates this component. The hooks then fire to update EntityGrid.
#[derive(Component, Debug, Default, Clone, Copy)]
#[component(on_insert = spatial_chunk_on_insert, on_remove = spatial_chunk_on_remove)]
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

/// Hook called when SpatialChunk is inserted.
/// Adds the entity to EntityGrid at its chunk position.
fn spatial_chunk_on_insert(mut world: DeferredWorld<'_>, context: HookContext) {
    let entity = context.entity;
    let Some(chunk) = world.get::<SpatialChunk>(entity).copied() else {
        return;
    };
    if let Some(mut grid) = world.get_resource_mut::<crate::server::broadcast::EntityGrid>() {
        grid.insert(chunk.as_tuple(), entity);
    }
}

/// Hook called when SpatialChunk is removed.
/// Removes the entity from EntityGrid.
fn spatial_chunk_on_remove(mut world: DeferredWorld<'_>, context: HookContext) {
    let entity = context.entity;
    let Some(chunk) = world.get::<SpatialChunk>(entity).copied() else {
        return;
    };
    if let Some(mut grid) = world.get_resource_mut::<crate::server::broadcast::EntityGrid>() {
        grid.remove(chunk.as_tuple(), entity);
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
