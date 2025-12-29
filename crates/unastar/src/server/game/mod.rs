//! ECS-based game server.
//!
//! This module provides the pure ECS server implementation,
//! replacing the legacy Server struct.
//!
//! ## Module Structure
//! - `types` - Type definitions (SessionEntityMap, PlayerSpawnData, etc.)
//! - `join` - Player join packet sending
//! - `packets` - Packet handling dispatch
//! - `blocks` - Block breaking and animations
//! - `chunks` - Chunk/subchunk request handling
//! - `commands` - Command processing

mod blocks;
mod chunks;
mod commands;
mod content;
mod join;
mod lifecycle;
mod packets;
mod persistence;
mod player_management;
pub mod types;

use bevy_ecs::prelude::*;
use jolyne::WorldTemplate;
use jolyne::valentine::StartGamePacketDimension;
use jolyne::valentine::types::BlockCoordinates;
use jolyne::valentine::types::Vec3F;
use std::sync::Arc;
use tracing::{info, trace};

use crate::command::CommandRegistry;
use crate::config::PlayerDataStore;
use crate::ecs::{CleanupSet, EntityLogicSet, NetworkSendSet, UnastarEcs};
use crate::registry::{BiomeRegistry, BlockRegistry, EntityRegistry, ItemRegistry};
use crate::server::broadcast::{
    EntityGrid, broadcast_block_updates, broadcast_despawn_system, broadcast_movement_system,
    broadcast_spawn_system, cleanup_despawned_entities, sync_spatial_chunks, tick_block_breaking,
};
use crate::world::ecs::{
    BlockBroadcastEvent, ChunkLoadConfig, ChunkTickingState, PendingChunkGenerations,
    PlayerDespawnedEvent, PlayerSpawnedEvent, on_block_changed, register_chunk_systems,
};
use crate::world::{ChunkManager, WorldConfig};

// Re-export public types
pub use super::config::ServerConfig;
pub use types::{PlayerPersistenceData, PlayerSpawnData, SessionEntityMap};

/// The ECS-based game server.
pub struct GameServer {
    pub ecs: UnastarEcs,
    pub world_config: WorldConfig,
    pub world_template: Arc<WorldTemplate>,
    pub config: ServerConfig,
    pub commands: CommandRegistry,
    pub current_tick: u64,

    // Player data persistence (legacy)
    player_data_store: Option<Arc<PlayerDataStore>>,
    save_previous_position: bool,

    // New provider-based persistence
    player_provider: Option<Arc<dyn crate::storage::PlayerProvider>>,
    save_on_disconnect: bool,
    world_provider: Option<Arc<dyn crate::storage::WorldProvider>>,

    // Registries
    pub items: ItemRegistry,
    pub entities: EntityRegistry,
    pub biomes: BiomeRegistry,
    pub blocks: BlockRegistry,

    // Item entity ID counter for dropped items (starts at high value to avoid player ID conflicts)
    pub next_item_entity_id: i64,
}

impl GameServer {
    /// Create a new game server with default config.
    pub fn new() -> Self {
        Self::with_config(ServerConfig::default())
    }

    /// Create a new game server with custom config.
    pub fn with_config(config: ServerConfig) -> Self {
        // Load registries
        let mut items = ItemRegistry::new();
        items.load_vanilla();

        let mut entities = EntityRegistry::new();
        entities.load_vanilla();

        let mut biomes = BiomeRegistry::new();
        biomes.load_vanilla();

        let mut blocks = BlockRegistry::new();
        blocks.load_vanilla();

        // Build world template
        let mut world_template = WorldTemplate::default();
        world_template.start_game_template.player_position = Vec3F {
            x: 0.5,
            y: 17.0,
            z: 0.5,
        };
        world_template.start_game_template.spawn_position = BlockCoordinates { x: 0, y: 17, z: 0 };
        world_template.start_game_template.dimension = match config.world.dimension {
            1 => StartGamePacketDimension::Nether,
            2 => StartGamePacketDimension::End,
            _ => StartGamePacketDimension::Overworld,
        };
        world_template.item_registry = Arc::new(items.to_packet());
        world_template.biome_definitions = Arc::new(biomes.to_packet());
        world_template.available_entities =
            Arc::new(entities.to_available_entity_identifiers_packet());

        // Build creative content from items registry with block runtime IDs
        world_template.creative_content = Arc::new(Self::build_creative_content(&items, &blocks));

        // Populate block palette in StartGame packet to ensure correct ID mapping
        world_template.start_game_template.block_properties = blocks.to_block_properties();

        let world_template = Arc::new(world_template);

        // Store world config for dimension info
        let world_config = config.world;

        // Create ECS with ChunkManager resource
        let mut ecs = UnastarEcs::new();
        ecs.world_mut().insert_resource(SessionEntityMap::default());
        ecs.world_mut().insert_resource(EntityGrid::default());
        ecs.world_mut()
            .insert_resource(ChunkManager::new(world_config));

        // Add chunk loading configuration
        ecs.world_mut()
            .insert_resource(ChunkLoadConfig::from_server_config(&config));

        // Add async chunk generation tracking resource (Phase 1 perf optimization)
        ecs.world_mut().init_resource::<PendingChunkGenerations>();

        // Add chunk ticking state for reusable HashSet (Phase 2 perf optimization)
        ecs.world_mut().init_resource::<ChunkTickingState>();

        // Register BlockBroadcastEvent for batched block update broadcasting
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<BlockBroadcastEvent>>();

        // Register PlayerSpawnedEvent and PlayerDespawnedEvent for spawn/despawn broadcasting
        // These replace marker components (PendingSpawnBroadcast, PendingDespawnBroadcast)
        // to eliminate archetype changes during player spawn/despawn
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerSpawnedEvent>>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerDespawnedEvent>>();

        // Register BlockChanged observer for immediate game logic reactions
        ecs.world_mut().add_observer(on_block_changed);

        // Register chunk management systems
        register_chunk_systems(ecs.schedule_mut());

        // Register block breaking tick system (server-side cracking animation)
        ecs.schedule_mut()
            .add_systems(tick_block_breaking.in_set(EntityLogicSet));

        // Register broadcast systems in NetworkSendSet (Post-Simulation)
        // sync_spatial_chunks must run before broadcasts for accurate lookups
        // broadcast_block_updates reads BlockBroadcastEvent for batched block updates
        ecs.post_schedule_mut().add_systems(
            (
                sync_spatial_chunks,
                broadcast_spawn_system,
                broadcast_movement_system,
                broadcast_despawn_system,
                broadcast_block_updates,
            )
                .chain()
                .in_set(NetworkSendSet),
        );

        // Register cleanup system for despawning entities after broadcast
        // Component on_remove hooks (SpatialChunk, ChunkLoader) handle cleanup automatically
        ecs.post_schedule_mut()
            .add_systems(cleanup_despawned_entities.in_set(CleanupSet));

        info!(
            items = items.len(),
            entities = entities.len(),
            biomes = biomes.len(),
            blocks = blocks.len(),
            "Registries loaded"
        );

        Self {
            ecs,
            world_config,
            world_template,
            config,
            commands: CommandRegistry::with_defaults(),
            current_tick: 0,
            player_data_store: None,
            save_previous_position: false,
            player_provider: None,
            save_on_disconnect: false,
            world_provider: None,
            items,
            entities,
            biomes,
            blocks,
            next_item_entity_id: 100000, // Start at high value to avoid conflicts with player IDs
        }
    }

    /// Configure player data persistence (legacy).
    pub fn set_player_data_store(&mut self, store: Arc<PlayerDataStore>, save_previous: bool) {
        self.player_data_store = Some(store);
        self.save_previous_position = save_previous;
    }

    /// Set the player provider for new persistence system.
    pub fn set_player_provider(
        &mut self,
        provider: Arc<dyn crate::storage::PlayerProvider>,
        save_on_disconnect: bool,
    ) {
        self.player_provider = Some(provider);
        self.save_on_disconnect = save_on_disconnect;
    }

    /// Set the world provider for chunk persistence.
    pub fn set_world_provider(&mut self, provider: Arc<dyn crate::storage::WorldProvider>) {
        self.world_provider = Some(provider);
    }

    /// Get the world provider (for ChunkManager integration).
    pub fn world_provider(&self) -> Option<Arc<dyn crate::storage::WorldProvider>> {
        self.world_provider.clone()
    }

    /// Run one tick of game logic.
    pub fn tick(&mut self) {
        self.current_tick += 1;
        self.ecs.tick();
        self.trace_tick();
    }

    /// Run the simulation phase of the tick (Physics, Logic, Chunk).
    pub fn tick_simulation(&mut self) {
        self.current_tick += 1;
        self.ecs.tick_simulation();
    }

    /// Run the post-simulation phase of the tick (Network, Cleanup).
    pub fn tick_post_simulation(&mut self) {
        // Dispatch actions from plugins before broadcast
        self.dispatch_actions();
        
        self.ecs.tick_post_simulation();
        self.trace_tick();
    }

    fn trace_tick(&self) {
        // Trace tick periodically
        if self.current_tick % 100 == 0 {
            let player_count = self
                .ecs
                .world()
                .get_resource::<SessionEntityMap>()
                .map(|m| m.len())
                .unwrap_or(0);
            trace!(tick = self.current_tick, players = player_count, "Tick");
        }
    }
}

impl Default for GameServer {
    fn default() -> Self {
        Self::new()
    }
}
