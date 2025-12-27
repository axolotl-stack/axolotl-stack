//! Chunk ECS components, resources, and systems.
//!
//! Chunks are ECS entities with components for global state (loaded, dirty, ticking).
//! - `ChunkViewers` tracks player sessions with this chunk in their view radius.
//! - `ChunkEntities` tracks non-player entities physically inside the chunk.
//! - Per-player sent chunks are tracked via `ChunkLoader` on player entities.
//!
//! ## Systems
//! - `initialize_chunk_loaders` - Sets up ChunkLoader for new players
//! - `update_chunk_loaders` - Handles player movement across chunks
//! - `process_chunk_load_queues` - Sends chunks to players center-outward
//! - `schedule_chunk_unloads` - Marks chunks with no viewers for unload
//! - `process_chunk_unloads` - Despawns chunks after grace period
//! - `cleanup_disconnecting_player_views` - Cleans up when player leaves

pub mod components;
pub mod loader;
pub mod manager;
pub mod systems;

pub use components::*;
pub use loader::ChunkLoader;
pub use manager::{ChunkManager, ChunkManagerWorldExt};
pub use systems::{
    ChunkLoadConfig, LastPublisherState, PlayerDisconnecting, broadcast_block_update,
    register_chunk_systems, world_to_chunk_coords, world_to_local_coords,
};
