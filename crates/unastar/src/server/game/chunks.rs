//! Chunk request handling.
//!
//! Contains subchunk request and chunk radius request handling.

use bevy_ecs::entity::Entity;
use tracing::{debug, trace};

use super::GameServer;
use crate::entity::components::{ChunkRadius, PlayerSession};
use crate::world::ChunkPos;
use crate::world::chunk::HeightMapType;
use crate::world::ecs::{ChunkManager, ChunkViewers};
use jolyne::protocol::packets::{
    PacketChunkRadiusUpdate, PacketRequestChunkRadius, PacketSubchunk, PacketSubchunkEntries,
    PacketSubchunkRequest,
};
use jolyne::protocol::types::{
    HeightMapDataType, McpePacket, SubChunkEntryWithoutCachingItem,
    SubChunkEntryWithoutCachingItemResult, Vec3I,
};

/// Maximum subchunk requests per packet (DoS protection).
const MAX_SUBCHUNK_REQUESTS: usize = 1024;

impl GameServer {
    /// Handle a chunk radius request from a player.
    pub(super) fn handle_chunk_radius_request(
        &mut self,
        entity: Entity,
        req: &PacketRequestChunkRadius,
    ) {
        let server_max = self.config.max_chunk_radius.max(1);
        let client_max = i32::from(req.max_radius).max(1);
        let effective_max = server_max.min(client_max);
        let requested = req.chunk_radius.max(1);
        let radius = requested.min(effective_max);

        {
            let world = self.ecs.world_mut();
            if let Some(mut chunk_radius) = world.get_mut::<ChunkRadius>(entity) {
                chunk_radius.0 = radius;
            }
        }

        // Send response
        let session = self.ecs.world().get::<PlayerSession>(entity);
        if let Some(session) = session {
            let _ = session.send(McpePacket::from(PacketChunkRadiusUpdate {
                chunk_radius: radius,
            }));
        }
    }

    /// Handle a subchunk request from a player.
    pub(super) fn handle_subchunk_request(&mut self, entity: Entity, req: &PacketSubchunkRequest) {
        let session_id = {
            let world = self.ecs.world();
            world
                .get::<PlayerSession>(entity)
                .map(|s| s.session_id)
                .unwrap_or(0)
        };

        let origin = &req.origin;
        let chunk_x = origin.x;
        let chunk_z = origin.z;

        // Only log at trace level to avoid overhead
        trace!(
            session_id,
            origin = ?origin,
            request_count = req.requests.len(),
            "SubChunkRequest received"
        );

        // Cap subchunk requests per packet (DoS protection)
        // Track unique chunks we serve so we can register player as viewer
        let mut served_chunks: std::collections::HashSet<(i32, i32)> =
            std::collections::HashSet::new();
        let mut entries = Vec::with_capacity(req.requests.len().min(MAX_SUBCHUNK_REQUESTS));

        for offset in req.requests.iter().take(MAX_SUBCHUNK_REQUESTS) {
            let sub_y = origin.y + offset.dy as i32;
            let target_chunk_x = chunk_x + offset.dx as i32;
            let target_chunk_z = chunk_z + offset.dz as i32;

            let chunk_pos = ChunkPos::new(target_chunk_x, target_chunk_z);
            served_chunks.insert((target_chunk_x, target_chunk_z));

            let min_sub_y = crate::world::chunk::MIN_Y >> 4;
            let max_sub_y = min_sub_y + (crate::world::SUBCHUNK_COUNT as i32) - 1;

            if sub_y < min_sub_y || sub_y > max_sub_y {
                entries.push(SubChunkEntryWithoutCachingItem {
                    dx: offset.dx,
                    dy: offset.dy,
                    dz: offset.dz,
                    result: SubChunkEntryWithoutCachingItemResult::YIndexOutOfBounds,
                    payload: vec![],
                    heightmap_type: HeightMapDataType::TooLow,
                    heightmap: None,
                    render_heightmap_type: HeightMapDataType::TooLow,
                    render_heightmap: None,
                });
                continue;
            }

            // Get chunk data from ECS ChunkData component (source of truth)
            let chunk_entity = {
                let world = self.ecs.world();
                let chunk_manager = world
                    .get_resource::<ChunkManager>()
                    .expect("ChunkManager resource must exist");
                chunk_manager.get_by_coords(chunk_pos.x, chunk_pos.z)
            };

            let Some(chunk_entity) = chunk_entity else {
                // Chunk doesn't exist yet - return empty success
                entries.push(SubChunkEntryWithoutCachingItem {
                    dx: offset.dx,
                    dy: offset.dy,
                    dz: offset.dz,
                    result: SubChunkEntryWithoutCachingItemResult::SuccessAllAir,
                    payload: vec![],
                    heightmap_type: HeightMapDataType::TooLow,
                    heightmap: None,
                    render_heightmap_type: HeightMapDataType::TooLow,
                    render_heightmap: None,
                });
                continue;
            };

            let (is_empty, subchunk_data, hm_type, hm_data) = {
                let world = self.ecs.world();
                let chunk_manager = world.get_resource::<ChunkManager>().expect("ChunkManager must exist");
                
                if let Some(chunk_data) = world.get::<crate::world::ecs::ChunkData>(chunk_entity) {
                    let is_empty = chunk_data.inner.is_subchunk_empty(sub_y);
                    let subchunk_data = if !is_empty {
                        // Check if we can use superflat cache
                        let use_cache = matches!(
                            chunk_manager.world_config().generator,
                            crate::world::WorldGenerator::SuperFlat
                        );
                        
                        if use_cache {
                            if let Some(cached) = crate::world::generator::flat::get_cached_superflat_subchunk(sub_y) {
                                cached.to_vec()
                            } else {
                                chunk_data.inner.encode_subchunk(sub_y).unwrap_or_default()
                            }
                        } else {
                            chunk_data.inner.encode_subchunk(sub_y).unwrap_or_default()
                        }
                    } else {
                        vec![]
                    };
                    let (ht, hm) = chunk_data.inner.get_subchunk_heightmap(sub_y);
                    let hm_type = match ht {
                        HeightMapType::TooHigh => HeightMapDataType::TooHigh,
                        HeightMapType::TooLow => HeightMapDataType::TooLow,
                        HeightMapType::HasData => HeightMapDataType::HasData,
                    };
                    (is_empty, subchunk_data, hm_type, hm)
                } else {
                    // Entity exists but component not ready (flushing commands)
                    // Fallback to generating a temporary chunk to avoid returning "all air"
                    trace!(chunk = ?(target_chunk_x, target_chunk_z), sub_y, "SubChunkRequest for unflushed chunk - using temporary generation");
                    let temp_chunk = chunk_manager.generate_chunk(target_chunk_x, target_chunk_z);
                    
                    let is_empty = temp_chunk.is_subchunk_empty(sub_y);
                    let subchunk_data = if !is_empty {
                        // Check if we can use superflat cache (even for temporary chunks)
                        let use_cache = matches!(
                            chunk_manager.world_config().generator,
                            crate::world::WorldGenerator::SuperFlat
                        );
                        
                        if use_cache {
                            if let Some(cached) = crate::world::generator::flat::get_cached_superflat_subchunk(sub_y) {
                                cached.to_vec()
                            } else {
                                temp_chunk.encode_subchunk(sub_y).unwrap_or_default()
                            }
                        } else {
                            temp_chunk.encode_subchunk(sub_y).unwrap_or_default()
                        }
                    } else {
                        vec![]
                    };
                    let (ht, hm) = temp_chunk.get_subchunk_heightmap(sub_y);
                    let hm_type = match ht {
                        HeightMapType::TooHigh => HeightMapDataType::TooHigh,
                        HeightMapType::TooLow => HeightMapDataType::TooLow,
                        HeightMapType::HasData => HeightMapDataType::HasData,
                    };
                    (is_empty, subchunk_data, hm_type, hm)
                }
            };

            let result = if is_empty {
                SubChunkEntryWithoutCachingItemResult::SuccessAllAir
            } else {
                SubChunkEntryWithoutCachingItemResult::Success
            };

            entries.push(SubChunkEntryWithoutCachingItem {
                dx: offset.dx,
                dy: offset.dy,
                dz: offset.dz,
                result,
                payload: subchunk_data,
                heightmap_type: hm_type,
                heightmap: hm_data.clone(),
                render_heightmap_type: hm_type,
                render_heightmap: hm_data,
            });
        }

        // Add player as viewer for served chunks (don't generate missing chunks here!)
        // The chunk streaming system will handle chunk generation asynchronously
        // First collect existing chunk entities, then add viewers
        let chunk_entities: Vec<_> = {
            let world = self.ecs.world();
            if let Some(chunk_manager) = world.get_resource::<crate::world::ecs::ChunkManager>() {
                served_chunks
                    .iter()
                    .filter_map(|(cx, cz)| chunk_manager.get_by_coords(*cx, *cz))
                    .collect()
            } else {
                Vec::new()
            }
        };
        
        // Now add player as viewer (can mutate world)
        for chunk_entity in chunk_entities {
            let world = self.ecs.world_mut();
            if let Some(mut viewers) = world.get_mut::<ChunkViewers>(chunk_entity) {
                viewers.insert(entity);
            }
        }

        let response = PacketSubchunk {
            dimension: req.dimension,
            origin: Vec3I {
                x: origin.x,
                y: origin.y,
                z: origin.z,
            },
            entries: PacketSubchunkEntries::SubChunkEntryWithoutCaching(entries),
        };

        let world = self.ecs.world();
        if let Some(session) = world.get::<PlayerSession>(entity) {
            if let Err(e) = session.send(McpePacket::from(response)) {
                debug!(session_id, error = ?e, "Failed to send SubChunk response");
            }
        }
    }
}
