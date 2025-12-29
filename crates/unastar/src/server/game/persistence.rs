use super::GameServer;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{GameMode, PlayerUuid};
use crate::server::game::SessionEntityMap;
use crate::config::{PlayerLastPosition, SpawnLocation};
use crate::network::SessionId;
use crate::server::game::types::PlayerPersistenceData;
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkStateFlags};
use bevy_ecs::prelude::*;
use std::sync::Arc;
use tracing::{info, warn};

impl GameServer {
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
    /// After saving, clears the DIRTY flag from saved chunks.
    pub async fn save_all_chunks(&mut self) -> usize {
        let provider = match &self.world_provider {
            Some(p) => p.clone(),
            None => return 0,
        };

        // Collect modified chunks from ECS using a proper query (check dirty flag)
        let chunks_to_save: Vec<(Entity, i32, i32, crate::storage::ChunkColumn)> = {
            let world = self.ecs.world_mut();

            // Use QueryState to query entities with all components and filter by dirty flag
            let mut query = world.query::<(Entity, &ChunkPosition, &ChunkData, &ChunkStateFlags)>();

            query
                .iter(&world)
                .filter(|(_, _, _, state)| state.is_dirty())
                .map(|(entity, pos, data, _)| {
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

        // Clear dirty flag from successfully saved chunks
        {
            let world = self.ecs.world_mut();
            for entity in saved_entities {
                // Entity may have been despawned during async save
                if let Some(mut state_flags) = world.get_mut::<ChunkStateFlags>(entity) {
                    state_flags.clear_dirty();
                }
            }
        }

        if count > 0 {
            info!(count, "Saved all modified chunks");
        }
        count
    }

    /// Get player data for persistence.
    pub(super) fn get_player_persistence_data(&self, session_id: SessionId) -> Option<PlayerPersistenceData> {
        let session_map = self.ecs.world().get_resource::<SessionEntityMap>()?;
        let entity = session_map.get(session_id)?;

        let world = self.ecs.world();
        let session = world.get::<crate::entity::components::PlayerSession>(entity)?;
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
}
