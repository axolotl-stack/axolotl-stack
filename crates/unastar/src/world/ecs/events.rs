//! Block change events for the ECS world.
//!
//! This module provides two event types for block modifications:
//! - `BlockChanged`: Immediate trigger for game logic (physics, lighting, redstone)
//! - `BlockBroadcastEvent`: Batched event for network broadcasting
//!
//! ## Architecture
//!
//! Block changes follow a three-consumer pattern:
//! 1. **Game Logic** (immediate): Observers react to `BlockChanged` synchronously
//! 2. **Network** (batched): `BlockBroadcastEvent` collected per-tick for efficient packets
//! 3. **Persistence** (lazy): ChunkStateFlags::DIRTY marked for eventual save

use bevy_ecs::prelude::*;
use glam::IVec3;

/// Triggered immediately when a block changes.
///
/// Observers react to this synchronously within the same tick.
/// Used for game logic that requires immediate causality:
/// - Block physics (sand/gravel falling)
/// - Lighting updates
/// - Redstone signal propagation
///
/// Note: This is an observer trigger, not a standard event.
/// Use `commands.trigger(BlockChanged { ... })` to fire it.
#[derive(Event, Debug, Clone)]
pub struct BlockChanged {
    /// The chunk entity containing this block.
    pub chunk_entity: Entity,
    /// World position of the changed block.
    pub block_pos: IVec3,
    /// Previous block runtime ID (before the change).
    pub old_block: u32,
    /// New block runtime ID (after the change).
    pub new_block: u32,
}

/// Batched event for network broadcasting.
///
/// Collected per-tick and processed by `broadcast_block_updates` system
/// for efficient packet bundling. Multiple block changes in the same chunk
/// during a single tick are grouped into fewer network packets.
///
/// Note: This is a standard Bevy event, not an observer trigger.
/// Use `event_writer.send(BlockBroadcastEvent { ... })` to emit.
#[derive(Message, Debug, Clone)]
pub struct BlockBroadcastEvent {
    /// The chunk entity containing this block.
    pub chunk_entity: Entity,
    /// World position of the changed block.
    pub block_pos: IVec3,
    /// New block runtime ID.
    pub new_block: u32,
}

// =============================================================================
// Player Spawn/Despawn Events
// =============================================================================

use glam::DVec3;

/// Event emitted when a player spawns and needs to be broadcast to others.
///
/// Replaces the `PendingSpawnBroadcast` marker component, enabling:
/// - Batching if multiple players spawn in the same tick
/// - No archetype changes (events don't affect entity archetypes)
/// - Clear separation between spawning logic and broadcast logic
///
/// Note: This is a standard Bevy event, use `event_writer.send()` to emit.
#[derive(Message, Debug, Clone)]
pub struct PlayerSpawnedEvent {
    /// The player entity that spawned.
    pub entity: Entity,
    /// Player's spawn position.
    pub position: DVec3,
    /// Player's network runtime ID.
    pub runtime_id: i64,
}

/// Event emitted when a player despawns and needs removal broadcast.
///
/// Replaces the `PendingDespawnBroadcast` marker component, enabling:
/// - Batching if multiple players disconnect in the same tick
/// - No archetype changes
/// - Clear separation between despawn logic and broadcast logic
///
/// Note: This is a standard Bevy event, use `event_writer.send()` to emit.
#[derive(Message, Debug, Clone)]
pub struct PlayerDespawnedEvent {
    /// The player entity that despawned.
    pub entity: Entity,
    /// Player's network runtime ID for RemoveEntity packet.
    pub runtime_id: i64,
    /// Player's spatial chunk for grid cleanup (handled by SpatialChunk hook, but kept for reference).
    pub spatial_chunk: (i32, i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_changed_event() {
        let event = BlockChanged {
            chunk_entity: Entity::PLACEHOLDER,
            block_pos: IVec3::new(10, 64, -20),
            old_block: 1,
            new_block: 0,
        };
        assert_eq!(event.block_pos.x, 10);
        assert_eq!(event.old_block, 1);
        assert_eq!(event.new_block, 0);
    }

    #[test]
    fn test_block_broadcast_event() {
        let event = BlockBroadcastEvent {
            chunk_entity: Entity::PLACEHOLDER,
            block_pos: IVec3::new(5, 32, 15),
            new_block: 42,
        };
        assert_eq!(event.block_pos.y, 32);
        assert_eq!(event.new_block, 42);
    }

    #[test]
    fn test_player_spawned_event() {
        let event = PlayerSpawnedEvent {
            entity: Entity::PLACEHOLDER,
            position: DVec3::new(0.5, 64.0, 0.5),
            runtime_id: 123,
        };
        assert_eq!(event.runtime_id, 123);
        assert_eq!(event.position.y, 64.0);
    }

    #[test]
    fn test_player_despawned_event() {
        let event = PlayerDespawnedEvent {
            entity: Entity::PLACEHOLDER,
            runtime_id: 456,
            spatial_chunk: (1, 2),
        };
        assert_eq!(event.runtime_id, 456);
        assert_eq!(event.spatial_chunk, (1, 2));
    }
}
