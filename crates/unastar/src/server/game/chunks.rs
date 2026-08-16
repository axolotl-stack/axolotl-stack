//! Chunk request domain system.
//!
//! Processes SubchunkRequest and RequestChunkRadius packets.

use bevy_ecs::prelude::*;
use tracing::{debug, trace};

use super::packet_queues::{ChunkAction, ChunkPacketQueue};
use super::types::ServerConfigResource;
use crate::entity::components::{ChunkRadius, PlayerSession};
use crate::world::chunk::HeightMapType;
use crate::world::ecs::{ChunkData, ChunkManager, ChunkViewers};
use jolyne::valentine::types::{
    HeightMapDataType, SubChunkEntryWithoutCachingItem, SubChunkEntryWithoutCachingItemResult,
    Vec3I,
};
use jolyne::valentine::{
    ChunkRadiusUpdatePacket, McpePacket, SubchunkPacket, SubchunkPacketEntries,
};

/// Maximum subchunk requests per packet (DoS protection).
const MAX_SUBCHUNK_REQUESTS: usize = 1024;

/// ECS system: drain chunk packet queue and process requests.
///
/// Runs in `PacketApplySet`. Parallel with movement and inventory because
/// it writes disjoint components (ChunkRadius, ChunkViewers).
pub fn apply_chunk_requests(
    mut queue: ResMut<ChunkPacketQueue>,
    config: Res<ServerConfigResource>,
    chunk_manager: Res<ChunkManager>,
    sessions: Query<&PlayerSession>,
    mut chunk_radii: Query<&mut ChunkRadius>,
    chunk_data_q: Query<&ChunkData>,
    mut chunk_viewers_q: Query<&mut ChunkViewers>,
) {
    let _span = tracing::info_span!("apply_chunk_requests", count = queue.0.len()).entered();
    for (entity, action) in queue.0.drain(..) {
        match action {
            ChunkAction::RadiusRequest(req) => {
                handle_chunk_radius_request(entity, &req, &config, &sessions, &mut chunk_radii);
            }
            ChunkAction::SubchunkRequest(req) => {
                handle_subchunk_request(
                    entity,
                    &req,
                    &chunk_manager,
                    &sessions,
                    &chunk_data_q,
                    &mut chunk_viewers_q,
                );
            }
        }
    }
}

fn handle_chunk_radius_request(
    entity: Entity,
    req: &jolyne::valentine::RequestChunkRadiusPacket,
    config: &Res<ServerConfigResource>,
    sessions: &Query<&PlayerSession>,
    chunk_radii: &mut Query<&mut ChunkRadius>,
) {
    let server_max = config.0.max_chunk_radius.max(1);
    let client_max = i32::from(req.max_radius).max(1);
    let effective_max = server_max.min(client_max);
    let requested = req.chunk_radius.max(1);
    let radius = requested.min(effective_max);

    if let Ok(mut chunk_radius) = chunk_radii.get_mut(entity) {
        chunk_radius.0 = radius;
    }

    if let Ok(session) = sessions.get(entity) {
        let _ = session.send(McpePacket::from(ChunkRadiusUpdatePacket {
            chunk_radius: radius,
        }));
    }
}

fn handle_subchunk_request(
    entity: Entity,
    req: &jolyne::valentine::SubchunkRequestPacket,
    chunk_manager: &Res<ChunkManager>,
    sessions: &Query<&PlayerSession>,
    chunk_data_q: &Query<&ChunkData>,
    chunk_viewers_q: &mut Query<&mut ChunkViewers>,
) {
    let session_id = sessions.get(entity).map(|s| s.session_id).unwrap_or(0);

    let origin = &req.origin;
    let chunk_x = origin.x;
    let chunk_z = origin.z;

    trace!(
        session_id,
        origin = ?origin,
        request_count = req.requests.len(),
        "SubChunkRequest received"
    );

    let mut served_chunks: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    let mut entries = Vec::with_capacity(req.requests.len().min(MAX_SUBCHUNK_REQUESTS));

    for offset in req.requests.iter().take(MAX_SUBCHUNK_REQUESTS) {
        let sub_y = origin.y + offset.y as i32;
        let target_chunk_x = chunk_x + offset.x as i32;
        let target_chunk_z = chunk_z + offset.z as i32;

        served_chunks.insert((target_chunk_x, target_chunk_z));

        let min_sub_y = crate::world::chunk::MIN_Y >> 4;
        let max_sub_y = min_sub_y + (crate::world::SUBCHUNK_COUNT as i32) - 1;

        if sub_y < min_sub_y || sub_y > max_sub_y {
            entries.push(SubChunkEntryWithoutCachingItem {
                dx: offset.x,
                dy: offset.y,
                dz: offset.z,
                result: SubChunkEntryWithoutCachingItemResult::YIndexOutOfBounds,
                payload: vec![],
                heightmap_type: HeightMapDataType::TooLow,
                heightmap: None,
                render_heightmap_type: HeightMapDataType::TooLow,
                render_heightmap: None,
            });
            continue;
        }

        let chunk_entity = chunk_manager.get_by_coords(target_chunk_x, target_chunk_z);

        let Some(chunk_entity) = chunk_entity else {
            entries.push(SubChunkEntryWithoutCachingItem {
                dx: offset.x,
                dy: offset.y,
                dz: offset.z,
                result: SubChunkEntryWithoutCachingItemResult::SuccessAllAir,
                payload: vec![],
                heightmap_type: HeightMapDataType::TooLow,
                heightmap: None,
                render_heightmap_type: HeightMapDataType::TooLow,
                render_heightmap: None,
            });
            continue;
        };

        let Ok(chunk_data) = chunk_data_q.get(chunk_entity) else {
            trace!(chunk = ?(target_chunk_x, target_chunk_z), "SubChunkRequest for loading chunk");
            entries.push(SubChunkEntryWithoutCachingItem {
                dx: offset.x,
                dy: offset.y,
                dz: offset.z,
                result: SubChunkEntryWithoutCachingItemResult::ChunkNotFound,
                payload: vec![],
                heightmap_type: HeightMapDataType::TooLow,
                heightmap: None,
                render_heightmap_type: HeightMapDataType::TooLow,
                render_heightmap: None,
            });
            continue;
        };

        let is_empty = chunk_data.inner.is_subchunk_empty(sub_y);
        let subchunk_data = if !is_empty {
            let use_cache = matches!(
                chunk_manager.world_config().generator,
                crate::world::WorldGenerator::SuperFlat
            );

            if use_cache {
                if let Some(cached) =
                    crate::world::generator::flat::get_cached_superflat_subchunk(sub_y)
                {
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

        let result = if is_empty {
            SubChunkEntryWithoutCachingItemResult::SuccessAllAir
        } else {
            SubChunkEntryWithoutCachingItemResult::Success
        };

        entries.push(SubChunkEntryWithoutCachingItem {
            dx: offset.x,
            dy: offset.y,
            dz: offset.z,
            result,
            payload: subchunk_data,
            heightmap_type: hm_type,
            heightmap: hm,
            render_heightmap_type: hm_type,
            render_heightmap: hm,
        });
    }

    // Add player as viewer for served chunks
    for (cx, cz) in &served_chunks {
        if let Some(chunk_entity) = chunk_manager.get_by_coords(*cx, *cz)
            && let Ok(mut viewers) = chunk_viewers_q.get_mut(chunk_entity)
        {
            viewers.insert(entity);
        }
    }

    let response = SubchunkPacket {
        dimension: req.dimension,
        origin: Vec3I {
            x: origin.x,
            y: origin.y,
            z: origin.z,
        },
        entries: SubchunkPacketEntries::SubChunkEntryWithoutCaching(entries),
    };

    if let Ok(session) = sessions.get(entity)
        && !session.send(McpePacket::from(response))
    {
        debug!(session_id, "Failed to send SubChunk response");
    }
}
