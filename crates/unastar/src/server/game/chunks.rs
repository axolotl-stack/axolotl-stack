//! Chunk request domain system.
//!
//! Processes SubchunkRequest and RequestChunkRadius packets.

use std::time::Instant;

use bevy_ecs::prelude::*;
use tracing::{debug, trace};

use super::packet_queues::{ChunkAction, ChunkPacketQueue};
use super::types::ServerConfigResource;
use crate::entity::components::{ChunkRadius, PlayerSession};
use crate::world::ecs::{
    ChunkData, ChunkLoader, ChunkManager, ChunkNetCache, ChunkViewers, ColumnLoads, StreamStats,
};
use jolyne::valentine::types::{
    EnumsSubChunkPacketPayloadHeightMapDataType, EnumsSubChunkPacketPayloadSubChunkRequestResult,
    SubChunkPacketPayloadHeightmapData, SubChunkPacketPayloadHeightmapDataRenderHeightMapType,
    SubChunkPacketPayloadSubChunkPacketData, SubChunkPacketPayloadSubChunkPosOffset, SubChunkPos,
};
use jolyne::valentine::{ChunkRadiusUpdatedPacket, McpePacket, SubChunkPacket};

/// Maximum subchunk requests per packet (DoS protection).
const MAX_SUBCHUNK_REQUESTS: usize = 1024;

/// Sub-chunk payload bytes per `SubChunk` packet. A request naming many
/// columns is answered in several packets (same center, entries in request
/// order) so no packet nears the RakNet reassembly limit (512 split parts,
/// about 700 KB), past which a peer drops the message and the ordered channel
/// stalls.
const SUBCHUNK_ANSWER_BYTES: usize = 128 * 1024;

/// ECS system: drain chunk packet queue and process requests.
///
/// Runs in `PacketApplySet`. Parallel with movement and inventory because
/// it writes disjoint components (ChunkRadius, ChunkViewers, ChunkLoader).
#[allow(clippy::too_many_arguments)]
pub fn apply_chunk_requests(
    mut queue: ResMut<ChunkPacketQueue>,
    config: Res<ServerConfigResource>,
    chunk_manager: Res<ChunkManager>,
    mut loads: ResMut<ColumnLoads>,
    stats: Res<StreamStats>,
    sessions: Query<&PlayerSession>,
    mut chunk_radii: Query<&mut ChunkRadius>,
    mut loaders: Query<&mut ChunkLoader>,
    mut chunk_data_q: Query<(&ChunkData, Option<&mut ChunkNetCache>)>,
    mut chunk_viewers_q: Query<&mut ChunkViewers>,
) {
    let _span = tracing::info_span!("apply_chunk_requests", count = queue.0.len()).entered();
    // Requests that wait (an edited column being encoded again, or the
    // player's transport holding its terrain budget) are retried next tick;
    // once one of a player's requests waits, the later ones wait too, so the
    // player's requests are answered in order.
    let mut deferred = Vec::new();
    let mut waiting: Vec<Entity> = Vec::new();
    for (entity, action) in queue.0.drain(..) {
        match action {
            ChunkAction::RadiusRequest(req) => {
                handle_chunk_radius_request(entity, &req, &config, &sessions, &mut chunk_radii);
            }
            ChunkAction::SubchunkRequest(req) => {
                let rest = if waiting.contains(&entity) {
                    Some(req)
                } else {
                    handle_subchunk_request(
                        entity,
                        &req,
                        config.0.block_network_ids,
                        &chunk_manager,
                        &mut loads,
                        &stats,
                        &sessions,
                        &mut loaders,
                        &mut chunk_data_q,
                        &mut chunk_viewers_q,
                    )
                };
                if let Some(rest) = rest {
                    waiting.push(entity);
                    deferred.push((entity, ChunkAction::SubchunkRequest(rest)));
                }
            }
        }
    }
    queue.0.extend(deferred);
}

fn handle_chunk_radius_request(
    entity: Entity,
    req: &jolyne::valentine::RequestChunkRadiusPacket,
    config: &Res<ServerConfigResource>,
    sessions: &Query<&PlayerSession>,
    chunk_radii: &mut Query<&mut ChunkRadius>,
) {
    let radius = granted_chunk_radius(
        req.chunk_radius,
        req.max_chunk_radius,
        config.0.max_chunk_radius,
    );

    if let Ok(mut chunk_radius) = chunk_radii.get_mut(entity) {
        chunk_radius.0 = radius;
    }

    if let Ok(session) = sessions.get(entity) {
        let _ = session.send(McpePacket::from(ChunkRadiusUpdatedPacket {
            chunk_radius: radius,
        }));
    }
}

/// The radius granted for a `RequestChunkRadius`: the requested radius, capped by
/// the server's `max_chunk_radius` and the client's own maximum, each at least 1.
fn granted_chunk_radius(requested: i32, client_max: u8, server_max: i32) -> i32 {
    requested
        .max(1)
        .min(server_max.max(1))
        .min(i32::from(client_max).max(1))
}

/// Answers a `SubChunkRequest` from the columns' shared encodings, column by
/// column in request order, and acknowledges the answered columns in the
/// player's send window. Returns what must wait for a later tick, if
/// anything:
///
/// - the whole request while a named column is being encoded again after an
///   edit, so an answer never shows a column as it was before an edit the
///   client was already told about;
/// - the columns whose answers no longer fit the terrain bytes the player's
///   transport may hold (see [`crate::world::ecs::SendWindow`]). One request
///   can name 1024 sub-chunks, megabytes of dense terrain, so a request is
///   split between columns rather than answered whole.
#[allow(clippy::too_many_arguments)]
fn handle_subchunk_request(
    entity: Entity,
    req: &jolyne::valentine::SubChunkRequestPacket,
    block_ids: crate::registry::block::BlockNetworkIds,
    chunk_manager: &ChunkManager,
    loads: &mut ColumnLoads,
    stats: &StreamStats,
    sessions: &Query<&PlayerSession>,
    loaders: &mut Query<&mut ChunkLoader>,
    chunk_data_q: &mut Query<(&ChunkData, Option<&mut ChunkNetCache>)>,
    chunk_viewers_q: &mut Query<&mut ChunkViewers>,
) -> Option<jolyne::valentine::SubChunkRequestPacket> {
    let started = Instant::now();
    let session_id = sessions.get(entity).map(|s| s.session_id).unwrap_or(0);

    let origin = &req.center_pos;
    let chunk_x = origin.subchunk_position_x;
    let chunk_z = origin.subchunk_position_z;

    trace!(
        session_id,
        origin = ?origin,
        request_count = req.sub_chunk_position_offset_list.len(),
        "SubChunkRequest received"
    );

    let offsets = &req.sub_chunk_position_offset_list[..req
        .sub_chunk_position_offset_list
        .len()
        .min(MAX_SUBCHUNK_REQUESTS)];
    let column_of = |offset: &SubChunkPacketPayloadSubChunkPosOffset| {
        (
            chunk_x + offset.subchunk_offset_x as i32,
            chunk_z + offset.subchunk_offset_z as i32,
        )
    };

    // Every named column, looked up once, in request order.
    let mut columns: std::collections::HashMap<(i32, i32), ColumnLookup> =
        std::collections::HashMap::new();
    let mut order = Vec::new();
    for offset in offsets {
        let target = column_of(offset);
        if columns.contains_key(&target) {
            continue;
        }
        let lookup = match chunk_manager.get_by_coords(target.0, target.1) {
            None => ColumnLookup::Absent,
            Some(chunk_entity) => match chunk_data_q.get_mut(chunk_entity) {
                Ok((data, Some(mut cache))) => match cache.current(
                    target,
                    &data.inner,
                    loads,
                    chunk_manager.runtime(),
                    block_ids,
                    stats,
                ) {
                    Some(encoded) => ColumnLookup::Ready(encoded),
                    None => return Some(req.clone()),
                },
                // A chunk entity spawned without the streaming components
                // (built by hand in tools and tests) is encoded for the answer.
                Ok((data, None)) => ColumnLookup::Ready(std::sync::Arc::new(
                    crate::world::ecs::EncodedColumn::encode(&data.inner, block_ids),
                )),
                Err(_) => ColumnLookup::Loading,
            },
        };
        columns.insert(target, lookup);
        order.push(target);
    }

    // The columns whose terrain fits what the transport may hold, in order.
    let now = Instant::now();
    let mut loader = loaders.get_mut(entity).ok();
    let transport_queued = sessions
        .get(entity)
        .map_or(0, PlayerSession::transport_queued_bytes);
    let mut fitting = 0;
    for target in &order {
        let bytes = match &columns[target] {
            ColumnLookup::Ready(encoded) => encoded.wire_bytes(),
            _ => 0,
        };
        if let Some(loader) = loader.as_mut() {
            if !loader.window_mut().may_answer(bytes, transport_queued, now) {
                break;
            }
            loader.window_mut().answered(bytes, now);
        }
        fitting += 1;
    }
    if fitting == 0 {
        return Some(req.clone());
    }
    let answering: std::collections::HashSet<(i32, i32)> =
        order[..fitting].iter().copied().collect();
    let rest: Vec<_> = offsets
        .iter()
        .filter(|offset| !answering.contains(&column_of(offset)))
        .cloned()
        .collect();

    let min_sub_y = crate::world::chunk::MIN_Y >> 4;
    let max_sub_y = min_sub_y + (crate::world::SUBCHUNK_COUNT as i32) - 1;
    let mut entries = Vec::with_capacity(offsets.len() - rest.len());
    for offset in offsets {
        let target = column_of(offset);
        if !answering.contains(&target) {
            continue;
        }
        let sub_y = origin.subchunk_position_y + offset.subchunk_offset_y as i32;
        if sub_y < min_sub_y || sub_y > max_sub_y {
            entries.push(empty_entry(
                offset.clone(),
                EnumsSubChunkPacketPayloadSubChunkRequestResult::Indexoutofbounds,
            ));
            continue;
        }
        let entry = match &columns[&target] {
            ColumnLookup::Ready(encoded) => encoded
                .subchunk_entry(sub_y, offset.clone())
                .expect("sub-chunk index checked against the column height"),
            ColumnLookup::Loading => {
                trace!(chunk = ?target, "SubChunkRequest for loading chunk");
                empty_entry(
                    offset.clone(),
                    EnumsSubChunkPacketPayloadSubChunkRequestResult::Levelchunkdoesntexist,
                )
            }
            // Not in memory: an empty column.
            ColumnLookup::Absent => empty_entry(
                offset.clone(),
                EnumsSubChunkPacketPayloadSubChunkRequestResult::Successallair,
            ),
        };
        entries.push(entry);
    }

    // Add player as viewer for served chunks and acknowledge them.
    for &(cx, cz) in &order[..fitting] {
        if let Some(chunk_entity) = chunk_manager.get_by_coords(cx, cz)
            && let Ok(mut viewers) = chunk_viewers_q.get_mut(chunk_entity)
        {
            viewers.insert(entity);
        }
        if let Some(loader) = loader.as_mut()
            && let Some(delay) = loader.window_mut().ack((cx, cz), now)
        {
            stats.acked.record(delay);
        }
    }

    if let Ok(session) = sessions.get(entity) {
        for sub_chunk_data in split_answer(entries) {
            let response = SubChunkPacket {
                cache_enabled: false,
                dimension_type: req.dimension_type.clone(),
                center_pos: SubChunkPos {
                    subchunk_position_x: origin.subchunk_position_x,
                    subchunk_position_y: origin.subchunk_position_y,
                    subchunk_position_z: origin.subchunk_position_z,
                },
                sub_chunk_data,
            };
            if !session.send(McpePacket::from(response)) {
                debug!(session_id, "Failed to send SubChunk response");
            }
        }
    }
    stats.answered.record(started.elapsed());
    (!rest.is_empty()).then(|| jolyne::valentine::SubChunkRequestPacket {
        sub_chunk_position_offset_list: rest,
        ..req.clone()
    })
}

/// Groups a request's answers into packets of about
/// [`SUBCHUNK_ANSWER_BYTES`] of payload, in request order, ending only between
/// columns so each column's answer arrives whole.
fn split_answer(
    entries: Vec<SubChunkPacketPayloadSubChunkPacketData>,
) -> Vec<Vec<SubChunkPacketPayloadSubChunkPacketData>> {
    let column_of = |entry: &SubChunkPacketPayloadSubChunkPacketData| {
        (
            entry.sub_chunk_pos_offset.subchunk_offset_x,
            entry.sub_chunk_pos_offset.subchunk_offset_z,
        )
    };
    let mut packets = Vec::new();
    let mut packet = Vec::new();
    let mut packet_bytes = 0;
    let mut entries = entries.into_iter().peekable();
    while let Some(entry) = entries.next() {
        packet_bytes += entry.serialized_sub_chunk.as_ref().map_or(0, |p| p.len());
        let column = column_of(&entry);
        packet.push(entry);
        let last = match entries.peek() {
            None => true,
            Some(next) => packet_bytes >= SUBCHUNK_ANSWER_BYTES && column_of(next) != column,
        };
        if last {
            packets.push(std::mem::take(&mut packet));
            packet_bytes = 0;
        }
    }
    packets
}

/// One requested column, looked up once per request.
enum ColumnLookup {
    Absent,
    /// Spawned this tick, its data not applied yet.
    Loading,
    Ready(std::sync::Arc<crate::world::ecs::EncodedColumn>),
}

fn empty_entry(
    offset: jolyne::valentine::types::SubChunkPacketPayloadSubChunkPosOffset,
    result: EnumsSubChunkPacketPayloadSubChunkRequestResult,
) -> SubChunkPacketPayloadSubChunkPacketData {
    SubChunkPacketPayloadSubChunkPacketData {
        sub_chunk_pos_offset: offset,
        sub_chunk_request_result: result,
        serialized_sub_chunk: None,
        height_map_data: SubChunkPacketPayloadHeightmapData {
            height_map_type: EnumsSubChunkPacketPayloadHeightMapDataType::Alltoolow,
            subchunk_height_map: None,
            render_height_map_type:
                SubChunkPacketPayloadHeightmapDataRenderHeightMapType::Alltoolow,
            subchunk_render_height_map: None,
        },
        blob_id: None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn chunk_radius_requests_are_clamped() {
        use super::granted_chunk_radius;
        // (requested, client max, server max) -> granted
        for (requested, client_max, server_max, granted) in [
            (8, 16, 12, 8),
            (64, 255, 12, 12),
            (10, 6, 12, 6),
            (0, 16, 12, 1),
            (-4, 16, 12, 1),
            (8, 0, 12, 1),
            (8, 16, 0, 1),
        ] {
            assert_eq!(
                granted_chunk_radius(requested, client_max, server_max),
                granted,
                "request {requested}, client max {client_max}, server max {server_max}"
            );
        }
    }

    use super::*;
    use crate::world::chunk::blocks;
    use crate::world::ecs::ChunkManagerWorldExt;
    use crate::world::{WorldConfig, WorldGenerator};
    use bevy_ecs::system::RunSystemOnce;
    use jolyne::valentine::types::{DimensionType, SubChunkPacketPayloadSubChunkPosOffset};
    use jolyne::valentine::{McpePacketData, SubChunkRequestPacket};

    /// An edited superflat chunk must be served as edited, not as the generator template.
    #[test]
    fn edited_superflat_subchunk_is_served_live() {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(WorldConfig {
            generator: WorldGenerator::SuperFlat,
            ..WorldConfig::default()
        }));
        world.insert_resource(ServerConfigResource(Default::default()));
        world.init_resource::<ChunkPacketQueue>();
        world.init_resource::<StreamStats>();
        world.init_resource::<ColumnLoads>();
        let chunk = world.get_or_create_chunk(0, 0).unwrap();
        // Replace the grass at y=3 with stone.
        let edited = {
            let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
            data.inner.set_block(1, 3, 1, *blocks::STONE);
            data.inner
                .encode_subchunk(0, crate::registry::block::BlockNetworkIds::Hashed)
                .unwrap()
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let player = world
            .spawn(PlayerSession::new(1, "test".into(), None, None, tx))
            .id();
        world.resource_mut::<ChunkPacketQueue>().0.push((
            player,
            ChunkAction::SubchunkRequest(SubChunkRequestPacket {
                dimension_type: DimensionType { value: 0 },
                sub_chunk_position_offset_list: vec![SubChunkPacketPayloadSubChunkPosOffset {
                    subchunk_offset_x: 0,
                    subchunk_offset_y: 0,
                    subchunk_offset_z: 0,
                }],
                center_pos: SubChunkPos {
                    subchunk_position_x: 0,
                    subchunk_position_y: 0,
                    subchunk_position_z: 0,
                },
            }),
        ));
        // The edited chunk is encoded again in the background; the request is
        // answered once that finished, never from the old encoding.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let answer = loop {
            world.run_system_once(apply_chunk_requests).unwrap();
            if let Ok(answer) = rx.try_recv() {
                break answer;
            }
            assert!(std::time::Instant::now() < deadline, "never answered");
            std::thread::sleep(std::time::Duration::from_millis(2));
            world
                .run_system_once(crate::world::ecs::systems::apply_loaded_columns)
                .unwrap();
        };
        let McpePacketData::SubChunkPacket(packet) = answer.data else {
            panic!("expected a SubChunk packet");
        };
        let served = packet.sub_chunk_data[0]
            .serialized_sub_chunk
            .as_ref()
            .expect("non-empty sub-chunk");
        assert_eq!(served.as_ref(), edited.as_slice());
        // The generator template differs, so a template cache would fail this test.
        let template = crate::world::generator::Generator::generate_proto(
            &crate::world::generator::FlatGenerator,
            crate::world::ChunkPos::new(0, 0),
        );
        assert_ne!(
            template
                .chunk
                .encode_subchunk(0, crate::registry::block::BlockNetworkIds::Hashed)
                .unwrap(),
            edited
        );
    }

    /// A request for many heavy columns is answered in several packets that
    /// keep request order, each under the size bound plus one column, and
    /// never cut a column in two.
    #[test]
    fn large_answers_split_between_columns() {
        let entry = |x: i8, y: i8, bytes: usize| SubChunkPacketPayloadSubChunkPacketData {
            sub_chunk_pos_offset: SubChunkPacketPayloadSubChunkPosOffset {
                subchunk_offset_x: x,
                subchunk_offset_y: y,
                subchunk_offset_z: 0,
            },
            sub_chunk_request_result: EnumsSubChunkPacketPayloadSubChunkRequestResult::Success,
            serialized_sub_chunk: Some(vec![0u8; bytes].into()),
            height_map_data: Default::default(),
            blob_id: None,
        };
        // 10 columns of 24 sub-chunks of 2 KiB: 48 KiB per column.
        let entries: Vec<_> = (0..10)
            .flat_map(|x| (0..24).map(move |y| entry(x, y, 2048)))
            .collect();
        let order: Vec<_> = entries
            .iter()
            .map(|e| e.sub_chunk_pos_offset.clone())
            .collect();
        let packets = split_answer(entries);
        assert!(packets.len() > 1);
        let column_bytes = 24 * 2048;
        for packet in &packets {
            let bytes: usize = packet
                .iter()
                .map(|e| e.serialized_sub_chunk.as_ref().unwrap().len())
                .sum();
            assert!(bytes < SUBCHUNK_ANSWER_BYTES + column_bytes, "{bytes}");
            assert_eq!(packet.len() % 24, 0, "a column was cut");
        }
        let flattened: Vec<_> = packets
            .into_iter()
            .flatten()
            .map(|e| e.sub_chunk_pos_offset)
            .collect();
        assert_eq!(flattened, order);

        // A small answer stays one packet.
        assert_eq!(
            split_answer(vec![entry(0, 0, 10), entry(1, 0, 10)]).len(),
            1
        );
    }
}
