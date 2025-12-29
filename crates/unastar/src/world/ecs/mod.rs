//! Chunk ECS components, resources, and systems.
//!
//! Chunks are ECS entities with components for global state (loaded, dirty, ticking).
//! - `ChunkViewers` tracks player sessions with this chunk in their view radius.
//! - `ChunkEntities` tracks non-player entities physically inside the chunk.
//! - Per-player sent chunks are tracked via `ChunkLoader` on player entities.
//!
//! ## Systems
//! - `update_chunk_loaders` - Handles player movement across chunks
//! - `process_chunk_load_queues` - Sends chunks to players center-outward
//! - `schedule_chunk_unloads` - Marks chunks with no viewers for unload
//! - `process_chunk_unloads` - Despawns chunks after grace period
//!
//! Note: `initialize_chunk_loaders` has been removed. ChunkLoader and LastPublisherState
//! are now included in PlayerBundle at spawn time to avoid archetype changes.
//!
//! ## Events
//! - `BlockChanged` - Observer trigger for immediate game logic (physics, lighting)
//! - `BlockBroadcastEvent` - Batched event for network broadcasting
//! - `PlayerSpawnedEvent` - Batched event for player spawn broadcasting
//! - `PlayerDespawnedEvent` - Batched event for player despawn broadcasting
//!
//! ## Automatic Cleanup
//! ChunkLoader has an on_remove hook that automatically removes the player from
//! all ChunkViewers when the entity is despawned. No explicit cleanup system needed.

pub mod components;
pub mod events;
pub mod generation_worker;
pub mod loader;
pub mod manager;
pub mod systems;

pub use components::{
    ChunkData, ChunkEntities, ChunkFlags, ChunkPendingUnload, ChunkPosition, ChunkState,
    ChunkStateFlags, ChunkTickingState, ChunkViewers, PendingChunkGenerations, PendingGeneration,
};
pub use events::{BlockBroadcastEvent, BlockChanged, PlayerDespawnedEvent, PlayerSpawnedEvent};
pub use loader::ChunkLoader;
pub use manager::{ChunkManager, ChunkManagerWorldExt};
pub use systems::{
    ChunkLoadConfig, LastPublisherState, broadcast_block_update, on_block_changed,
    register_chunk_systems, world_to_chunk_coords, world_to_local_coords,
};
