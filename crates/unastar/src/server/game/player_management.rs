use super::GameServer;
use crate::entity::bundles::PlayerBundle;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, BreakingState, ChunkRadius, CursorItem, GameMode, HeldSlot, InventoryOpened,
    ItemStackRequestState, LastBroadcastPosition, MainInventory, OffhandSlot, Player, PlayerInput,
    PlayerName, PlayerSession, PlayerState, PlayerUuid, RuntimeEntityId, SpatialChunk,
};
use crate::network::SessionId;
use crate::server::game::{PlayerSpawnData, SessionEntityMap};
use crate::ecs::events::EventBuffer;
use crate::world::ecs::{
    ChunkLoader, LastPublisherState, PlayerDespawnedEvent, PlayerSpawnedEvent,
};
use bevy_ecs::prelude::*;
use tracing::info;
use unastar_api::PluginEvent;

impl GameServer {
    /// Spawn a player entity.
    pub fn spawn_player(&mut self, data: PlayerSpawnData) -> Entity {
        // ... (existing logic)
        let player_uuid = data
            .uuid
            .as_ref()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .unwrap_or_else(uuid::Uuid::new_v4);

        let player_name = data.display_name.clone();

        // Push PlayerJoin event to internal EventBuffer for plugins
        if let Some(mut event_buffer) = self.ecs.world_mut().get_resource_mut::<EventBuffer>() {
            event_buffer.push(PluginEvent::PlayerJoin {
                player_id: player_uuid.to_string(),
                username: player_name.clone(),
            });
        }

        let position = Position(data.position);
        let runtime_id = data.runtime_id;
        
        // ... (rest of the method)

        // Calculate chunk position for ChunkLoader initialization
        let chunk_x = (position.0.x / 16.0).floor() as i32;
        let chunk_z = (position.0.z / 16.0).floor() as i32;

        // Create and initialize ChunkLoader at player's position
        let mut chunk_loader = ChunkLoader::new(data.chunk_radius);
        chunk_loader.move_to(chunk_x, chunk_z);
        // If move_to was a no-op (same position), force reload
        if !chunk_loader.has_pending() {
            chunk_loader.force_reload();
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
                position: position.clone(),
                rotation: Rotation::default(),
                game_mode: self.config.default_gamemode,
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
                // Chunk streaming components (Phase 7: included at spawn to avoid archetype changes)
                chunk_loader,
                last_publisher_state: LastPublisherState {
                    // Use impossible values so first update always triggers
                    chunk_x: i32::MIN,
                    chunk_z: i32::MIN,
                    radius: 0,
                    queue_was_empty: false,
                },
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

        // Emit PlayerSpawnedEvent for broadcast system (replaces PendingSpawnBroadcast marker)
        self.ecs.world_mut().write_message(PlayerSpawnedEvent {
            entity,
            position: data.position,
            runtime_id,
        });

        // Send join packets
        self.send_join_packets(entity);

        // Note: Initial chunks are sent by the ECS chunk systems
        // (process_chunk_load_queues reads from ChunkLoader which is already initialized)

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

        // Get entity data for despawn event before removal
        let despawn_data: Option<(Entity, i64, (i32, i32))> = {
            let session_map = self.ecs.world().get_resource::<SessionEntityMap>();
            session_map.and_then(|map| {
                let entity = map.get(session_id)?;
                let world = self.ecs.world();
                let runtime_id = world.get::<RuntimeEntityId>(entity)?.0;
                let spatial = world.get::<SpatialChunk>(entity)?;
                Some((entity, runtime_id, spatial.as_tuple()))
            })
        };

        // Remove from session map
        {
            let mut session_map = match self.ecs.world_mut().get_resource_mut::<SessionEntityMap>()
            {
                Some(map) => map,
                None => return,
            };
            if session_map.remove(session_id).is_none() {
                return;
            }
        }

        let Some((entity, runtime_id, spatial_chunk)) = despawn_data else {
            return;
        };

        // Emit PlayerDespawnedEvent for broadcast system (replaces PendingDespawnBroadcast marker)
        // cleanup_despawned_entities in CleanupSet will despawn the entity
        // ChunkLoader's on_remove hook automatically cleans up chunk viewers
        self.ecs.world_mut().write_message(PlayerDespawnedEvent {
            entity,
            runtime_id,
            spatial_chunk,
        });

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
}
