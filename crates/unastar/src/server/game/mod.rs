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
use jolyne::valentine::EnumsGameType;
use jolyne::valentine::types::{BlockPos, Vec3};
use std::sync::Arc;
use tracing::{info, warn};

use crate::command::CommandRegistry;
use crate::ecs::{
    CleanupSet, EntityLogicSet, NetworkSendSet, PacketApplySet, PhysicsSet, UnastarEcs,
};
use crate::entity::bundles::PlayerBundle;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, BreakingState, ChunkRadius, CursorItem, GameMode, HeldSlot, InventoryOpened,
    ItemStackRequestState, LastBroadcastPosition, MainInventory, OffhandSlot, Player, PlayerInput,
    PlayerName, PlayerSession, PlayerState, PlayerUuid, RuntimeEntityId, SpatialChunk,
};
use crate::entity::systems::{
    apply_drag, apply_gravity, apply_poison, apply_regeneration, apply_velocity, apply_wither,
    check_ground_collision, clamp_velocity, despawn_dead, tick_age, tick_air_supply,
    tick_damage_immunity, tick_effects, tick_fire, tick_item_despawn, tick_item_pickup_delay,
    tick_mob_age, tick_projectile_lifetime,
};
use crate::network::SessionId;
use crate::registry::{BiomeRegistry, BlockRegistry, EntityRegistry, ItemRegistry};
use crate::server::broadcast::{
    EntityGrid, broadcast_block_updates, broadcast_despawn_system, broadcast_movement_system,
    broadcast_spawn_system, cleanup_despawned_entities, sync_spatial_chunks, tick_block_breaking,
};
use crate::storage::{ChunkColumn, PlayerData};
use crate::world::ChunkManager;
use crate::world::ecs::{
    BlockBroadcastEvent, ChunkData, ChunkLoadConfig, ChunkLoader, ChunkPosition, ChunkStateFlags,
    ChunkTickingState, LastPublisherState, PendingChunkGenerations, PlayerDespawnedEvent,
    PlayerSpawnedEvent, on_block_changed, register_chunk_systems,
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

fn game_mode_to_storage_id(game_mode: GameMode) -> u8 {
    match game_mode {
        GameMode::Survival => 0,
        GameMode::Creative => 1,
        GameMode::Adventure => 2,
        GameMode::Spectator => 6,
    }
}

fn player_record(world: &World, entity: Entity) -> Option<(uuid::Uuid, PlayerData)> {
    use crate::entity::components::{Experience, Health, Hunger};
    let uuid = world.get::<PlayerUuid>(entity)?.0;
    let position = world.get::<Position>(entity)?;
    let rotation = world.get::<Rotation>(entity)?;
    let health = world.get::<Health>(entity).cloned().unwrap_or_default();
    let hunger = world.get::<Hunger>(entity).cloned().unwrap_or_default();
    let experience = world.get::<Experience>(entity).cloned().unwrap_or_default();
    let inventory = world.get::<MainInventory>(entity);
    let armour = world.get::<ArmourInventory>(entity);
    Some((
        uuid,
        PlayerData {
            uuid: uuid.to_string(),
            position: position.0.to_array(),
            rotation: [rotation.yaw, rotation.pitch],
            dimension: world.resource::<ServerConfigResource>().0.world.dimension,
            game_mode: game_mode_to_storage_id(*world.get::<GameMode>(entity)?),
            health: health.current,
            max_health: health.max,
            food: hunger.food_level,
            saturation: hunger.saturation,
            exhaustion: hunger.exhaustion,
            experience: experience.level,
            experience_progress: experience.progress,
            inventory: inventory
                .map(|inventory| inventory.0.slots().iter().map(Into::into).collect())
                .unwrap_or_default(),
            armour: std::array::from_fn(|i| {
                armour
                    .map(|armour| (&armour.slots()[i]).into())
                    .unwrap_or_default()
            }),
            offhand: world
                .get::<OffhandSlot>(entity)
                .map(|slot| (&slot.0).into())
                .unwrap_or_default(),
            cursor: world
                .get::<CursorItem>(entity)
                .map(|slot| (&slot.0).into())
                .unwrap_or_default(),
            held_slot: world
                .get::<HeldSlot>(entity)
                .map(|slot| slot.0)
                .unwrap_or(0),
        },
    ))
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
        world_template.start_game_template.position = Vec3 {
            x: 0.5,
            y: 17.0,
            z: 0.5,
        };
        world_template
            .start_game_template
            .settings
            .default_spawn_block_position = BlockPos { x: 0, y: 17, z: 0 };
        world_template
            .start_game_template
            .settings
            .spawn_settings
            .dimension = config.world.dimension;
        world_template.start_game_template.game_type = EnumsGameType::Survival;
        world_template.start_game_template.settings.game_type = EnumsGameType::Survival;
        world_template
            .start_game_template
            .enable_item_stack_net_manager = true;
        world_template
            .start_game_template
            .movement_settings
            .server_authoritative_block_breaking = true;
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
            (
                apply_gravity,
                clamp_velocity,
                apply_velocity,
                check_ground_collision,
                apply_drag,
            )
                .chain()
                .in_set(PhysicsSet),
        );

        ecs.schedule_mut().add_systems(
            (tick_block_breaking, plugins::process_plugin_actions).in_set(EntityLogicSet),
        );
        ecs.schedule_mut().add_systems(
            (
                tick_age,
                tick_mob_age,
                apply_regeneration,
                apply_poison,
                apply_wither,
                tick_air_supply,
                tick_fire,
                tick_damage_immunity,
                tick_effects,
                tick_item_pickup_delay,
                tick_item_despawn,
                tick_projectile_lifetime,
                despawn_dead,
            )
                .chain()
                .in_set(EntityLogicSet)
                .before(tick_block_breaking)
                .before(plugins::process_plugin_actions),
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

    pub async fn save_all_players(&mut self) -> usize {
        let records = {
            let world = self.ecs.world_mut();
            let mut query = world.query_filtered::<Entity, With<PlayerUuid>>();
            query
                .iter(world)
                .filter_map(|entity| player_record(world, entity))
                .collect::<Vec<_>>()
        };
        let mut saved = 0;
        for (uuid, data) in records {
            if self.save_player_record(uuid, &data).await {
                saved += 1;
            }
        }
        saved
    }

    pub async fn save_all_chunks(&mut self) -> usize {
        let Some((provider, dimension)) =
            self.ecs
                .world()
                .get_resource::<ChunkManager>()
                .and_then(|manager| {
                    manager
                        .provider()
                        .map(|provider| (provider, manager.dimension()))
                })
        else {
            return 0;
        };

        let records = {
            let world = self.ecs.world_mut();
            let mut query = world.query::<(Entity, &ChunkPosition, &ChunkData, &ChunkStateFlags)>();
            query
                .iter(world)
                .filter(|(_, _, _, flags)| flags.is_dirty())
                .map(|(entity, position, data, _)| {
                    (
                        entity,
                        position.to_chunk_pos(),
                        ChunkColumn::new(data.inner.clone()),
                    )
                })
                .collect::<Vec<_>>()
        };

        let mut saved = 0;
        let mut saved_entities = Vec::new();
        for (entity, position, column) in records {
            match provider.save_column(position, dimension, &column).await {
                Ok(()) => {
                    saved += 1;
                    saved_entities.push(entity);
                }
                Err(e) => warn!(chunk = ?position, error = %e, "Failed to save chunk"),
            }
        }

        for entity in saved_entities {
            if let Some(mut flags) = self.ecs.world_mut().get_mut::<ChunkStateFlags>(entity) {
                flags.clear_dirty();
            }
        }

        saved
    }

    pub async fn save_player(&mut self, session_id: SessionId) -> bool {
        let record = {
            let world = self.ecs.world();
            world
                .resource::<SessionEntityMap>()
                .get(session_id)
                .and_then(|entity| player_record(world, entity))
        };
        let Some((uuid, data)) = record else {
            return false;
        };
        self.save_player_record(uuid, &data).await
    }

    async fn save_player_record(&self, uuid: uuid::Uuid, data: &PlayerData) -> bool {
        let provider = self
            .ecs
            .world()
            .get_resource::<types::PlayerProviderResource>()
            .map(|r| r.0.clone());
        let positions = self
            .ecs
            .world()
            .get_resource::<types::PlayerDataStoreResource>()
            .map(|r| r.0.clone());
        let mut saved = provider.is_some() || positions.is_some();
        if let Some(provider) = provider
            && let Err(error) = provider.save(uuid, data).await
        {
            warn!(%uuid, %error, "Failed to save player data");
            saved = false;
        }
        if let Some(positions) = positions {
            let last = crate::config::PlayerLastPosition {
                dimension: data.dimension,
                location: crate::config::SpawnLocation {
                    x: data.position[0] as f32,
                    y: data.position[1] as f32,
                    z: data.position[2] as f32,
                    yaw: data.rotation[0],
                    pitch: data.rotation[1],
                },
            };
            if let Err(error) = positions.save_last_position(&uuid.to_string(), &last).await {
                warn!(%uuid, %error, "Failed to save last player position");
                saved = false;
            }
        }
        saved
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

        let mut rotation = Rotation::default();
        let mut game_mode = default_gamemode;
        let mut main_inventory = MainInventory::default();
        let mut armour = ArmourInventory::default();
        let mut offhand = OffhandSlot::default();
        let mut cursor = CursorItem::default();
        let mut held_slot = HeldSlot::default();
        let mut health = crate::entity::components::Health::default();
        let mut hunger = crate::entity::components::Hunger::default();
        let mut experience = crate::entity::components::Experience::default();
        if let Some(saved) = data.saved_data {
            rotation = Rotation::new(saved.rotation[0], saved.rotation[1]);
            game_mode = match saved.game_mode {
                0 => GameMode::Survival,
                1 => GameMode::Creative,
                2 => GameMode::Adventure,
                6 => GameMode::Spectator,
                _ => unreachable!("saved player data was validated before spawning"),
            };
            for (slot, item) in main_inventory.0.slots_mut().iter_mut().zip(saved.inventory) {
                *slot = item.into();
            }
            let [helmet, chestplate, leggings, boots] = saved.armour.map(Into::into);
            armour.set_helmet(helmet).expect("validated saved helmet");
            armour
                .set_chestplate(chestplate)
                .expect("validated saved chestplate");
            armour
                .set_leggings(leggings)
                .expect("validated saved leggings");
            armour.set_boots(boots).expect("validated saved boots");
            offhand.0 = saved.offhand.into();
            cursor.0 = saved.cursor.into();
            held_slot.0 = saved.held_slot;
            health.current = saved.health;
            health.max = saved.max_health;
            hunger.food_level = saved.food;
            hunger.saturation = saved.saturation;
            hunger.exhaustion = saved.exhaustion;
            experience.level = saved.experience;
            experience.progress = saved.experience_progress;
        }

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
                rotation,
                game_mode,
                state: PlayerState::default(),
                input: PlayerInput::default(),
                chunk_radius: ChunkRadius(data.chunk_radius),
                breaking_state: BreakingState::default(),
                spatial_chunk: SpatialChunk::from_position(&position),
                last_broadcast: LastBroadcastPosition {
                    x: position.0.x,
                    y: position.0.y,
                    z: position.0.z,
                    yaw: rotation.yaw,
                    pitch: rotation.pitch,
                },
                chunk_loader,
                last_publisher_state: LastPublisherState::default(),
                main_inventory,
                armour,
                offhand,
                held_slot,
                cursor,
                inventory_opened: InventoryOpened::default(),
                item_stack_state: ItemStackRequestState::default(),
            })
            .id();
        self.ecs
            .world_mut()
            .entity_mut(entity)
            .insert((health, hunger, experience));

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
            CerealizerNetworkItemInstanceDescriptorSerializedData, CreativeContentPacket,
            CreativeGroupInfoPayload, CreativeItemEntryPayload,
            EnumsSharedTypesCreativeItemCategory, TypedServerNetIdstructCreativeItemNetIdTag,
        };

        let creative_data = CreativeInventoryData::load();

        let mut protocol_groups = Vec::new();
        let mut items_list = Vec::new();
        let mut entry_id_counter = 1u32;
        let mut global_group_index = 0u32;

        for (tab_name, tab_groups) in creative_data.all_groups_ordered() {
            let category_map = |tab_name: &str| match tab_name {
                "Construction" => EnumsSharedTypesCreativeItemCategory::Construction,
                "Nature" => EnumsSharedTypesCreativeItemCategory::Nature,
                "Equipment" => EnumsSharedTypesCreativeItemCategory::Equipment,
                _ => EnumsSharedTypesCreativeItemCategory::Items,
            };
            let category = category_map(tab_name);

            for group in tab_groups {
                protocol_groups.push(CreativeGroupInfoPayload {
                    creative_category: category,
                    name: group.group_name.to_string(),
                    group_icon_item: CerealizerNetworkItemInstanceDescriptorSerializedData::default(
                    ),
                });

                for creative_item in group.items {
                    let Some(item_entry) = items.get_by_name(creative_item.item_id()) else {
                        // Skip items not in the registry (internal state blocks, etc.)
                        continue;
                    };
                    let item_network_id = item_entry.network_id;

                    let block_runtime_id = blocks
                        .get_by_name(creative_item.item_id())
                        .map_or(0, |b| b.default_state_id as i32);

                    items_list.push(CreativeItemEntryPayload {
                        creative_net_id: TypedServerNetIdstructCreativeItemNetIdTag {
                            id: entry_id_counter,
                        },
                        item_instance: CerealizerNetworkItemInstanceDescriptorSerializedData {
                            id: item_network_id,
                            stacksize: 1,
                            auxvalue: creative_item.damage() as u32,
                            block_runtime_id,
                            user_data_buffer: Vec::new(),
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
            entries: items_list,
        }
    }
}

impl Default for GameServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::components::{
        Age, ChunkRadius, DroppedItem, Living, OnGround, PickupDelay, PlayerSession, Velocity,
    };
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;

    #[test]
    fn game_tick_runs_physics_set_for_living_entities() {
        let mut server = GameServer::new();
        let entity = server
            .ecs
            .world_mut()
            .spawn((
                Living,
                Position(DVec3::new(0.0, 10.0, 0.0)),
                Velocity(DVec3::ZERO),
                OnGround(false),
            ))
            .id();

        server.tick();

        let world = server.ecs.world();
        let position = world.get::<Position>(entity).expect("position");
        let velocity = world.get::<Velocity>(entity).expect("velocity");
        assert!(
            position.0.y < 10.0,
            "scheduled physics should move airborne living entities downward"
        );
        assert!(
            velocity.0.y < 0.0,
            "scheduled physics should leave falling velocity after drag"
        );
    }

    #[test]
    fn game_tick_runs_entity_lifecycle_systems() {
        let mut server = GameServer::new();
        let entity = server
            .ecs
            .world_mut()
            .spawn((Age(0), DroppedItem, PickupDelay(2)))
            .id();

        server.tick();

        let world = server.ecs.world();
        let age = world.get::<Age>(entity).expect("age");
        let pickup_delay = world.get::<PickupDelay>(entity).expect("pickup delay");
        assert_eq!(age.0, 1);
        assert_eq!(pickup_delay.0, 1);
    }

    #[test]
    fn player_spawn_sets_join_chunk_gate_without_network_io() {
        let mut server = GameServer::with_config(ServerConfig {
            default_chunk_radius: 2,
            max_chunk_radius: 4,
            ..ServerConfig::default()
        });
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(32);

        let entity = server.spawn_player(PlayerSpawnData {
            session_id: 42,
            display_name: "smoke".to_string(),
            xuid: None,
            uuid: Some("00000000-0000-0000-0000-000000000042".to_string()),
            runtime_id: 7,
            position: DVec3::new(0.5, 17.0, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 2,
        });

        {
            let world = server.ecs.world();
            let chunk_radius = world.get::<ChunkRadius>(entity).expect("chunk radius");
            assert_eq!(chunk_radius.0, 2);

            let chunk_loader = world.get::<ChunkLoader>(entity).expect("chunk loader");
            assert_eq!(chunk_loader.position(), (0, 0));
            assert_eq!(chunk_loader.queue_len(), 25);
            assert_eq!(chunk_loader.peek_next_to_load(), Some((0, 0)));
            assert!(
                chunk_loader.has_pending(),
                "spawn should leave chunk publish work queued for the join gate"
            );

            let session = world.get::<PlayerSession>(entity).expect("player session");
            assert_eq!(session.packets_dropped(), 0);
        }

        let first_packet = outbound_rx.try_recv().expect("initial join packet");
        match first_packet.data {
            McpePacketData::ChunkRadiusUpdatedPacket(packet) => {
                assert_eq!(packet.chunk_radius, 2);
            }
            other => panic!("expected ChunkRadiusUpdate first, got {other:?}"),
        }

        assert!(
            outbound_rx.try_recv().is_ok(),
            "spawn should enqueue additional join packets after chunk radius"
        );
    }
}
