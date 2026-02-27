//! ECS-based game server.
//!
//! This module provides the pure ECS server implementation,
//! replacing the legacy Server struct.

mod blocks;
mod chunks;
mod commands;
mod inventory;
mod join;
mod movement;
mod packet_domains;
mod packet_queues;
mod packets;
mod plugins;
pub mod types;

use bevy_ecs::prelude::*;
use jolyne::WorldTemplate;
use jolyne::valentine::StartGamePacketDimension;
use jolyne::valentine::types::{BlockCoordinates, Vec3F};
use std::sync::Arc;
use tracing::{info, warn};

use crate::command::CommandRegistry;
use crate::ecs::{CleanupSet, EntityLogicSet, NetworkSendSet, PacketApplySet, UnastarEcs};
use crate::entity::bundles::PlayerBundle;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, BreakingState, ChunkRadius, CursorItem, HeldSlot, InventoryOpened,
    ItemStackRequestState, LastBroadcastPosition, MainInventory, OffhandSlot, Player, PlayerInput,
    PlayerName, PlayerSession, PlayerState, PlayerUuid, RuntimeEntityId, SpatialChunk,
};
use crate::network::SessionId;
use crate::registry::{BiomeRegistry, BlockRegistry, EntityRegistry, ItemRegistry};
use crate::server::broadcast::{
    EntityGrid, broadcast_block_updates, broadcast_despawn_system, broadcast_movement_system,
    broadcast_spawn_system, cleanup_despawned_entities, sync_spatial_chunks, tick_block_breaking,
};
use crate::world::ChunkManager;
use crate::world::ecs::{
    BlockBroadcastEvent, ChunkLoadConfig, ChunkLoader, ChunkTickingState, LastPublisherState,
    PendingChunkGenerations, PlayerDespawnedEvent, PlayerSpawnedEvent, on_block_changed,
    register_chunk_systems,
};

// Re-export public types
pub use super::config::ServerConfig;
pub use types::{
    CommandRegistryResource, ItemEntityIdCounter, PlayerPersistenceData, PlayerSpawnData,
    ServerConfigResource, SessionEntityMap,
};

/// The ECS-based game server.
///
/// Thin wrapper around the ECS World and Schedule.
/// All game state lives inside the World as Resources.
pub struct GameServer {
    pub ecs: UnastarEcs,
}

impl GameServer {
    pub fn new() -> Self {
        Self::with_config(ServerConfig::default())
    }

    pub fn with_config(config: ServerConfig) -> Self {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let mut entities = EntityRegistry::new();
        entities.load_vanilla();
        let mut biomes = BiomeRegistry::new();
        biomes.load_vanilla();
        let mut blocks = BlockRegistry::new();
        blocks.load_vanilla();

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
        world_template.creative_content = Arc::new(Self::build_creative_content(&items, &blocks));
        world_template.start_game_template.block_properties = blocks.to_block_properties();

        let world_template = Arc::new(world_template);
        let world_config = config.world;
        let mut ecs = UnastarEcs::new();

        // Insert all game state as ECS Resources
        ecs.world_mut()
            .insert_resource(types::ServerWorldTemplate(world_template.clone()));
        ecs.world_mut()
            .insert_resource(ServerConfigResource(config.clone()));
        ecs.world_mut()
            .insert_resource(CommandRegistryResource(CommandRegistry::with_defaults()));
        ecs.world_mut().insert_resource(ItemEntityIdCounter(100000));
        ecs.world_mut().insert_resource(SessionEntityMap::default());
        ecs.world_mut().insert_resource(EntityGrid::default());
        ecs.world_mut()
            .insert_resource(ChunkManager::new(world_config));
        ecs.world_mut()
            .insert_resource(ChunkLoadConfig::from_server_config(&config));
        ecs.world_mut().init_resource::<PendingChunkGenerations>();
        ecs.world_mut().init_resource::<ChunkTickingState>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<BlockBroadcastEvent>>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerSpawnedEvent>>();
        ecs.world_mut()
            .init_resource::<bevy_ecs::message::Messages<PlayerDespawnedEvent>>();

        // Per-domain packet queues and event buffers
        ecs.world_mut()
            .init_resource::<packet_queues::MovementPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::BlockPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::InventoryPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::ChatPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::ChunkPacketQueue>();
        ecs.world_mut()
            .init_resource::<packet_queues::MovementEvents>();
        ecs.world_mut()
            .init_resource::<packet_queues::InventoryEvents>();
        ecs.world_mut().init_resource::<packet_queues::ChatEvents>();

        ecs.world_mut().add_observer(on_block_changed);
        register_chunk_systems(ecs.schedule_mut());

        // Domain systems in PacketApplySet (movement, inventory, chunks run parallel)
        ecs.schedule_mut().add_systems(
            (
                movement::apply_movement,
                inventory::apply_inventory,
                chunks::apply_chunk_requests,
                commands::apply_chat_and_commands,
            )
                .in_set(PacketApplySet),
        );
        // Exclusive block system runs after movement (which forwards block actions)
        ecs.schedule_mut().add_systems(
            blocks::apply_block_actions
                .in_set(PacketApplySet)
                .after(movement::apply_movement),
        );
        // Consolidate per-domain events into main EventBuffer after all domain systems
        ecs.schedule_mut().add_systems(
            packet_queues::consolidate_domain_events
                .in_set(PacketApplySet)
                .after(blocks::apply_block_actions)
                .after(inventory::apply_inventory)
                .after(chunks::apply_chunk_requests)
                .after(commands::apply_chat_and_commands),
        );

        ecs.schedule_mut().add_systems(
            (tick_block_breaking, plugins::process_plugin_actions).in_set(EntityLogicSet),
        );
        ecs.schedule_mut().add_systems(
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
        ecs.schedule_mut()
            .add_systems(cleanup_despawned_entities.in_set(CleanupSet));

        ecs.world_mut()
            .insert_resource(types::ItemRegistryResource(Arc::new(items)));
        ecs.world_mut()
            .insert_resource(types::BlockRegistryResource(Arc::new(blocks)));

        info!("Registries loaded");
        Self { ecs }
    }

    pub async fn save_all_players(&self) -> usize {
        // TODO: implement persistence
        0
    }

    pub async fn save_all_chunks(&mut self) -> usize {
        // TODO: implement persistence
        0
    }

    pub fn spawn_player(&mut self, data: PlayerSpawnData) -> Entity {
        let player_uuid = data
            .uuid
            .as_ref()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .unwrap_or_else(uuid::Uuid::new_v4);
        let player_name = data.display_name.clone();
        let position = Position(data.position);
        let runtime_id = data.runtime_id;
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;

        let mut chunk_loader = ChunkLoader::new(data.chunk_radius);
        chunk_loader.move_to(chunk_x, chunk_z);
        if !chunk_loader.has_pending() {
            chunk_loader.force_reload();
        }

        let default_gamemode = self
            .ecs
            .world()
            .get_resource::<ServerConfigResource>()
            .unwrap()
            .0
            .default_gamemode;

        let entity = self
            .ecs
            .world_mut()
            .spawn(PlayerBundle {
                player: Player,
                name: PlayerName(player_name),
                uuid: PlayerUuid(player_uuid),
                session: PlayerSession::new(
                    data.session_id,
                    data.display_name,
                    data.xuid,
                    data.uuid,
                    data.outbound_tx,
                ),
                runtime_id: RuntimeEntityId(runtime_id),
                position,
                rotation: Rotation::default(),
                game_mode: default_gamemode,
                state: PlayerState::default(),
                input: PlayerInput::default(),
                chunk_radius: ChunkRadius(data.chunk_radius),
                breaking_state: BreakingState::default(),
                spatial_chunk: SpatialChunk::from_position(&position),
                last_broadcast: LastBroadcastPosition {
                    x: position.0.x,
                    y: position.0.y,
                    z: position.0.z,
                    yaw: 0.0,
                    pitch: 0.0,
                },
                chunk_loader,
                last_publisher_state: LastPublisherState::default(),
                main_inventory: MainInventory::default(),
                armour: ArmourInventory::default(),
                offhand: OffhandSlot::default(),
                held_slot: HeldSlot::default(),
                cursor: CursorItem::default(),
                inventory_opened: InventoryOpened::default(),
                item_stack_state: ItemStackRequestState::default(),
            })
            .id();

        if let Some(mut session_map) = self.ecs.world_mut().get_resource_mut::<SessionEntityMap>() {
            session_map.insert(data.session_id, entity);
        }

        self.ecs.world_mut().write_message(PlayerSpawnedEvent {
            entity,
            position: data.position,
            runtime_id,
        });
        self.send_join_packets(entity);
        info!(session_id = data.session_id, "Player spawned as ECS entity");
        entity
    }

    pub fn despawn_player(&mut self, session_id: SessionId) {
        let entity = {
            if let Some(mut map) = self.ecs.world_mut().get_resource_mut::<SessionEntityMap>() {
                map.remove(session_id)
            } else {
                None
            }
        };
        if let Some(entity) = entity {
            self.ecs.world_mut().despawn(entity);
            info!(session_id, "Player despawned");
        }
    }

    pub fn tick(&mut self) {
        self.ecs.tick();
    }

    fn build_creative_content(
        items: &ItemRegistry,
        blocks: &BlockRegistry,
    ) -> jolyne::valentine::CreativeContentPacket {
        use crate::registry::CreativeInventoryData;
        use jolyne::valentine::{
            CreativeContentPacket, CreativeContentPacketGroupsItem,
            CreativeContentPacketGroupsItemCategory, CreativeContentPacketItemsItem, ItemLegacy,
            ItemLegacyContent, ItemLegacyContentExtra,
        };

        let creative_data = match CreativeInventoryData::load() {
            Ok(data) => data,
            Err(e) => {
                warn!(
                    "Failed to load creative inventory data: {}. Using empty creative content.",
                    e
                );
                return CreativeContentPacket::default();
            }
        };

        let mut protocol_groups = Vec::new();
        let mut items_list = Vec::new();
        let mut entry_id_counter = 1i32;
        let mut global_group_index = 0i32;

        for (tab_name, tab_groups) in creative_data.all_groups_ordered() {
            let category_map = |tab_name: &str| match tab_name {
                "Construction" => CreativeContentPacketGroupsItemCategory::Construction,
                "Nature" => CreativeContentPacketGroupsItemCategory::Nature,
                "Equipment" => CreativeContentPacketGroupsItemCategory::Equipment,
                _ => CreativeContentPacketGroupsItemCategory::Items,
            };
            let category = category_map(tab_name);

            for group in tab_groups {
                protocol_groups.push(CreativeContentPacketGroupsItem {
                    category,
                    name: group.group_name.clone(),
                    icon_item: ItemLegacy::default(),
                });

                for creative_item in &group.items {
                    let item_network_id = items
                        .get_by_name(creative_item.item_id())
                        .map(|item| item.id as i32)
                        .unwrap_or_else(|| {
                            warn!(
                                "Creative item not found in registry: {}",
                                creative_item.item_id()
                            );
                            1
                        });

                    let block_runtime_id = blocks
                        .get_by_name(creative_item.item_id())
                        .map_or(0, |b| b.default_state_id as i32);

                    items_list.push(CreativeContentPacketItemsItem {
                        entry_id: entry_id_counter,
                        item: ItemLegacy {
                            network_id: item_network_id,
                            content: Some(Box::new(ItemLegacyContent {
                                count: 1,
                                metadata: creative_item.damage().into(),
                                block_runtime_id,
                                extra: ItemLegacyContentExtra::default(),
                            })),
                        },
                        group_index: global_group_index,
                    });
                    entry_id_counter += 1;
                }
                global_group_index += 1;
            }
        }
        CreativeContentPacket {
            groups: protocol_groups,
            items: items_list,
        }
    }
}

impl Default for GameServer {
    fn default() -> Self {
        Self::new()
    }
}
