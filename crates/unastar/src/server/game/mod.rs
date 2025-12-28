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
mod join;
mod packets;
pub mod types;

use bevy_ecs::prelude::*;
use jolyne::WorldTemplate;
use jolyne::valentine::StartGamePacketDimension;
use jolyne::valentine::types::Vec3F;
use jolyne::valentine::types::BlockCoordinates;
use std::sync::Arc;
use tracing::{info, trace, warn};

use crate::command::CommandRegistry;
use crate::config::{PlayerDataStore, PlayerLastPosition, SpawnLocation};
use crate::ecs::{EntityLogicSet, NetworkSendSet, UnastarEcs};
use crate::entity::bundles::PlayerBundle;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, BreakingState, ChunkRadius, CursorItem, GameMode, HeldSlot, InventoryOpened,
    ItemStackRequestState, MainInventory, OffhandSlot, PendingDespawnBroadcast,
    PendingSpawnBroadcast, Player, PlayerInput, PlayerName, PlayerSession, PlayerState, PlayerUuid,
    RuntimeEntityId, SpatialChunk,
};
use crate::network::SessionId;
use crate::registry::{BiomeRegistry, BlockRegistry, EntityRegistry, ItemRegistry};
use crate::server::broadcast::{
    EntityGrid, broadcast_despawn_system, broadcast_movement_system, broadcast_spawn_system,
    tick_block_breaking, update_spatial_grid_system,
};
use crate::world::ecs::{ChunkLoadConfig, register_chunk_systems};
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

        // Register chunk management systems
        register_chunk_systems(ecs.schedule_mut());

        // Register block breaking tick system (server-side cracking animation)
        ecs.schedule_mut()
            .add_systems(tick_block_breaking.in_set(EntityLogicSet));

        // Register broadcast systems in NetworkSendSet
        // update_spatial_grid_system must run before broadcasts for accurate lookups
        ecs.schedule_mut().add_systems(
            (
                update_spatial_grid_system,
                broadcast_spawn_system,
                broadcast_movement_system,
                broadcast_despawn_system,
            )
                .chain()
                .in_set(NetworkSendSet),
        );

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

    /// Save all online players to the provider.
    ///
    /// Returns the number of players saved.
    pub async fn save_all_players(&self) -> usize {
        let provider = match &self.player_provider {
            Some(p) => p.clone(),
            None => return 0,
        };

        // Collect player data from ECS
        let players_to_save: Vec<(uuid::Uuid, crate::storage::PlayerData)> = {
            let world = self.ecs.world();
            let session_map = match world.get_resource::<SessionEntityMap>() {
                Some(map) => map,
                None => return 0,
            };

            session_map
                .iter()
                .filter_map(|(_, entity)| {
                    let position = world.get::<Position>(entity)?;
                    let rotation = world.get::<Rotation>(entity)?;
                    let uuid_comp = world.get::<PlayerUuid>(entity)?;
                    let game_mode = world.get::<GameMode>(entity)?;

                    let game_mode_u8 = match *game_mode {
                        GameMode::Survival => 0,
                        GameMode::Creative => 1,
                        GameMode::Adventure => 2,
                        GameMode::Spectator => 6, // Bedrock spectator mode ID
                    };

                    Some((
                        uuid_comp.0,
                        crate::storage::PlayerData {
                            uuid: uuid_comp.0.to_string(),
                            position: [position.0.x, position.0.y, position.0.z],
                            rotation: [rotation.yaw, rotation.pitch],
                            dimension: self.world_config.dimension,
                            game_mode: game_mode_u8,
                            health: 20.0,
                            food: 20,
                            experience: 0,
                        },
                    ))
                })
                .collect()
        };

        let count = players_to_save.len();

        // Save each player asynchronously
        for (uuid, data) in players_to_save {
            if let Err(e) = provider.save(uuid, &data).await {
                warn!(uuid = %uuid, error = %e, "Failed to save player data");
            }
        }

        info!(count, "Saved all player data");
        count
    }

    /// Save all modified chunks to the world provider.
    ///
    /// Returns the number of chunks saved.
    /// After saving, removes the `ChunkModified` marker from saved chunks.
    pub async fn save_all_chunks(&mut self) -> usize {
        use crate::world::ecs::{ChunkData, ChunkModified, ChunkPosition};

        let provider = match &self.world_provider {
            Some(p) => p.clone(),
            None => return 0,
        };

        // Collect modified chunks from ECS using a proper query
        let chunks_to_save: Vec<(Entity, i32, i32, crate::storage::ChunkColumn)> = {
            let world = self.ecs.world_mut();

            // Use QueryState to properly query entities with all three components
            let mut query =
                world.query_filtered::<(Entity, &ChunkPosition, &ChunkData), With<ChunkModified>>();

            query
                .iter(&world)
                .map(|(entity, pos, data)| {
                    let column = crate::storage::ChunkColumn::new(data.inner.clone());
                    (entity, pos.x, pos.z, column)
                })
                .collect()
        };

        let count = chunks_to_save.len();
        let mut saved_entities = Vec::with_capacity(count);

        // Save each chunk asynchronously
        let dim = self.world_config.dimension;
        for (entity, x, z, column) in chunks_to_save {
            let chunk_pos = crate::world::ChunkPos::new(x, z);
            if let Err(e) = provider.save_column(chunk_pos, dim, &column).await {
                warn!(x, z, error = %e, "Failed to save chunk");
            } else {
                saved_entities.push(entity);
            }
        }

        // Remove ChunkModified marker from successfully saved chunks
        {
            let world = self.ecs.world_mut();
            for entity in saved_entities {
                // Entity may have been despawned during async save
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    entity_mut.remove::<ChunkModified>();
                }
            }
        }

        if count > 0 {
            info!(count, "Saved all modified chunks");
        }
        count
    }

    /// Spawn a player entity.
    pub fn spawn_player(&mut self, data: PlayerSpawnData) -> Entity {
        // Parse UUID from string if available
        let player_uuid = data
            .uuid
            .as_ref()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .unwrap_or_else(uuid::Uuid::new_v4);

        let player_name = data.display_name.clone();

        let position = Position(data.position);
        let entity = self
            .ecs
            .world_mut()
            .spawn(PlayerBundle {
                player: Player,
                name: PlayerName(player_name),
                uuid: PlayerUuid(player_uuid),
                session: PlayerSession {
                    session_id: data.session_id,
                    display_name: data.display_name,
                    xuid: data.xuid,
                    uuid: data.uuid,
                    outbound_tx: data.outbound_tx,
                },
                runtime_id: RuntimeEntityId(data.runtime_id),
                position: position.clone(),
                rotation: Rotation::default(),
                game_mode: self.config.default_gamemode,
                state: PlayerState::default(),
                input: PlayerInput::default(),
                chunk_radius: ChunkRadius(data.chunk_radius),
                breaking_state: BreakingState::default(),
                spatial_chunk: SpatialChunk::from_position(&position),
                pending_spawn: PendingSpawnBroadcast,
                // Inventory components (all start empty for new players)
                main_inventory: MainInventory::default(),
                armour: ArmourInventory::default(),
                offhand: OffhandSlot::default(),
                held_slot: HeldSlot::default(),
                cursor: CursorItem::default(),
                inventory_opened: InventoryOpened::default(),
                item_stack_state: ItemStackRequestState::default(),
            })
            .id();

        // Update session map
        if let Some(mut session_map) = self.ecs.world_mut().get_resource_mut::<SessionEntityMap>() {
            session_map.insert(data.session_id, entity);
        }

        // Send join packets
        self.send_join_packets(entity);

        // Note: Initial chunks are sent by the ECS ChunkLoader system
        // (initialize_chunk_loaders -> process_chunk_load_queues)

        info!(session_id = data.session_id, "Player spawned as ECS entity");
        entity
    }

    /// Despawn a player entity.
    pub fn despawn_player(&mut self, session_id: SessionId) {
        // Get player data for persistence before despawning
        let player_data = self.get_player_persistence_data(session_id);

        // Get player UUID and provider data before removing from ECS
        let player_provider_data: Option<(uuid::Uuid, crate::storage::PlayerData)> = {
            let session_map = self.ecs.world().get_resource::<SessionEntityMap>();
            session_map.and_then(|map| {
                let entity = map.get(session_id)?;
                let world = self.ecs.world();
                let position = world.get::<Position>(entity)?;
                let rotation = world.get::<Rotation>(entity)?;
                let uuid_comp = world.get::<PlayerUuid>(entity)?;
                let game_mode = world.get::<GameMode>(entity)?;

                let game_mode_u8 = match *game_mode {
                    GameMode::Survival => 0,
                    GameMode::Creative => 1,
                    GameMode::Adventure => 2,
                    GameMode::Spectator => 6,
                };

                Some((
                    uuid_comp.0,
                    crate::storage::PlayerData {
                        uuid: uuid_comp.0.to_string(),
                        position: [position.0.x, position.0.y, position.0.z],
                        rotation: [rotation.yaw, rotation.pitch],
                        dimension: self.world_config.dimension,
                        game_mode: game_mode_u8,
                        health: 20.0,
                        food: 20,
                        experience: 0,
                    },
                ))
            })
        };

        // Remove from session map and get entity for processing
        let entity = {
            let mut session_map = match self.ecs.world_mut().get_resource_mut::<SessionEntityMap>()
            {
                Some(map) => map,
                None => return,
            };
            match session_map.remove(session_id) {
                Some(e) => e,
                None => return,
            }
        };

        // Mark entity for despawn broadcast and chunk cleanup
        // PlayerDisconnecting triggers cleanup_disconnecting_player_views system
        // PendingDespawnBroadcast triggers broadcast_despawn_system
        // The ECS systems will handle cleanup and despawn on next tick
        self.ecs.world_mut().entity_mut(entity).insert((
            PendingDespawnBroadcast,
            crate::world::ecs::PlayerDisconnecting,
        ));

        // Save position if configured (legacy TOML format)
        if self.save_previous_position {
            if let (Some(store), Some(data)) = (self.player_data_store.clone(), player_data) {
                tokio::spawn(async move {
                    let _ = store
                        .save_last_position(&data.uuid, &data.last_position)
                        .await;
                });
            }
        }

        // Save player data via LevelDB provider (if enabled)
        if self.save_on_disconnect {
            if let (Some(provider), Some((uuid, data))) =
                (self.player_provider.clone(), player_provider_data)
            {
                tokio::spawn(async move {
                    if let Err(e) = provider.save(uuid, &data).await {
                        tracing::warn!(uuid = %uuid, error = %e, "Failed to save player on disconnect");
                    }
                });
            }
        }

        info!(session_id, "Player despawned");
    }

    /// Get player data for persistence.
    fn get_player_persistence_data(&self, session_id: SessionId) -> Option<PlayerPersistenceData> {
        let session_map = self.ecs.world().get_resource::<SessionEntityMap>()?;
        let entity = session_map.get(session_id)?;

        let world = self.ecs.world();
        let session = world.get::<PlayerSession>(entity)?;
        let position = world.get::<Position>(entity)?;
        let rotation = world.get::<Rotation>(entity)?;

        let uuid = session.uuid.clone()?;

        Some(PlayerPersistenceData {
            uuid,
            last_position: PlayerLastPosition {
                dimension: self.world_config.dimension,
                location: SpawnLocation {
                    x: position.0.x as f32,
                    y: position.0.y as f32,
                    z: position.0.z as f32,
                    yaw: rotation.yaw,
                    pitch: rotation.pitch,
                },
            },
        })
    }

    /// Run one tick of game logic.
    pub fn tick(&mut self) {
        self.current_tick += 1;

        // Run ECS systems (handles chunk streaming, broadcasts, cleanup)
        self.ecs.tick();

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

    /// Build creative content packet from items registry.
    ///
    /// This creates an anonymous group (empty name, air icon) and adds all
    /// vanilla items to it for the creative inventory.
    /// For block items, looks up the block_runtime_id from the blocks registry.
    fn build_creative_content(
        items: &ItemRegistry,
        blocks: &BlockRegistry,
    ) -> jolyne::valentine::CreativeContentPacket {
        use crate::registry::RegistryEntry;
        use jolyne::valentine::{
            ItemLegacy, ItemLegacyContent, ItemLegacyContentExtra, CreativeContentPacket,
            CreativeContentPacketGroupsItem, CreativeContentPacketGroupsItemCategory,
            CreativeContentPacketItemsItem,
        };

        // Create proper groups like Dragonfly does
        // Each group needs: category, name (localized), icon_item
        // Anonymous groups (empty name) don't show a header
        let groups = vec![
            // Group 0: Construction blocks
            CreativeContentPacketGroupsItem {
                category: CreativeContentPacketGroupsItemCategory::Construction,
                name: "itemGroup.name.planks".to_string(),
                icon_item: ItemLegacy {
                    network_id: 0, // Air = use first item as icon
                    content: None,
                },
            },
            // Group 1: Nature blocks
            CreativeContentPacketGroupsItem {
                category: CreativeContentPacketGroupsItemCategory::Nature,
                name: "itemGroup.name.stone".to_string(),
                icon_item: ItemLegacy {
                    network_id: 0,
                    content: None,
                },
            },
            // Group 2: Equipment
            CreativeContentPacketGroupsItem {
                category: CreativeContentPacketGroupsItemCategory::Equipment,
                name: "itemGroup.name.sword".to_string(),
                icon_item: ItemLegacy {
                    network_id: 0,
                    content: None,
                },
            },
            // Group 3: Miscellaneous items
            CreativeContentPacketGroupsItem {
                category: CreativeContentPacketGroupsItemCategory::Items,
                name: "itemGroup.name.miscFood".to_string(),
                icon_item: ItemLegacy {
                    network_id: 0,
                    content: None,
                },
            },
        ];

        // Build items list from registry - entry_id must be sequential from 1
        let mut entry_id_counter = 1u32;
        let items_list: Vec<CreativeContentPacketItemsItem> = items
            .iter()
            .filter(|item| item.id() != 0) // Skip air (id 0)
            .map(|item| {
                let entry_id = entry_id_counter;
                entry_id_counter += 1;

                // Look up block_runtime_id if this item is a block
                let block_runtime_id = blocks
                    .get_by_name(item.string_id())
                    .map(|b| b.default_state_id as i32)
                    .unwrap_or(0);

                // Assign items to groups based on their type
                // For now, put blocks in group 1 (Nature/stone), tools in 2, others in 3
                let group_index = if block_runtime_id > 0 {
                    1 // Blocks go to Nature group
                } else {
                    let name = item.string_id();
                    if name.contains("sword")
                        || name.contains("pickaxe")
                        || name.contains("axe")
                        || name.contains("shovel")
                        || name.contains("hoe")
                        || name.contains("helmet")
                        || name.contains("chestplate")
                        || name.contains("leggings")
                        || name.contains("boots")
                    {
                        2 // Equipment
                    } else {
                        3 // Misc items
                    }
                };

                CreativeContentPacketItemsItem {
                    entry_id: entry_id as i32,
                    item: ItemLegacy {
                        network_id: item.id() as i32,
                        content: Some(Box::new(ItemLegacyContent {
                            count: 1,
                            metadata: 0,
                            block_runtime_id,
                            extra: ItemLegacyContentExtra::default(),
                        })),
                    },
                    group_index,
                }
            })
            .collect();

        tracing::info!(
            groups = groups.len(),
            items = items_list.len(),
            "Built creative content"
        );

        CreativeContentPacket {
            groups,
            items: items_list,
        }
    }
}

impl Default for GameServer {
    fn default() -> Self {
        Self::new()
    }
}
