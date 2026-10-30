//! A headless client for chunk streaming tests and benchmarks.
//!
//! [`SimPlayer`] joins a `GameServer` without a network: it reads the
//! packets the server queues for it, answers every `LevelChunk` in sub-chunk
//! request mode with a `SubChunkRequest` before the next tick (batched like a
//! client: one request per tick, relative to a nearby column, at most 1024
//! sub-chunks per request), and records which columns are complete.
//!
//! Its [`SimLink`] stands in for the RakNet session: everything the server
//! sends in a tick is compressed as one batch (deflate level 4, the server
//! default) and added to the link's queue, which drains a fixed number of
//! bytes per tick. The link reports that queue to the server like a real
//! session does, and remembers its peak, which must stay under the session's
//! 1 MiB limit (a session over it is disconnected).

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy_ecs::entity::Entity;
use glam::DVec3;
use jolyne::proto::batch::{BatchCodec, BatchCompression, BatchEncoding};
use jolyne::stream::transport::SendQueueProbe;
use jolyne::valentine::types::{
    DimensionType, SubChunkPacketPayloadSubChunkPosOffset, SubChunkPos,
};
use jolyne::valentine::{McpePacket, McpePacketArgs, McpePacketData, SubChunkRequestPacket};
use unastar::{GameServer, PlayerSpawnData};

/// The RakNet session's queue limit (`max_queued_reliable_bytes`).
pub const SESSION_QUEUE_LIMIT: usize = 1024 * 1024;
const SUBCHUNKS_PER_COLUMN: i32 = 24;
const LOWEST_SUBCHUNK: i32 = -4;
const MAX_REQUEST_ENTRIES: usize = 1024;

/// A link that drains `drain_per_tick` bytes per tick.
pub struct SimLink {
    queued: Arc<AtomicUsize>,
    drain_per_tick: usize,
    /// Most bytes ever queued, measured right after a tick's packets.
    pub peak: usize,
    codec: BatchCodec,
    scratch: bytes::BytesMut,
}

impl SimLink {
    pub fn new(drain_per_tick: usize) -> Self {
        Self {
            queued: Arc::new(AtomicUsize::new(0)),
            drain_per_tick,
            peak: 0,
            codec: BatchCodec::new(BatchEncoding {
                enabled: true,
                algorithm: BatchCompression::Deflate,
                level: 4,
                threshold: 1,
                prefix: true,
            })
            .expect("batch codec"),
            scratch: bytes::BytesMut::new(),
        }
    }

    pub fn probe(&self) -> SendQueueProbe {
        let queued = self.queued.clone();
        SendQueueProbe::new(move || queued.load(Ordering::Relaxed))
    }

    pub fn queued(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }

    /// The tick's packets enter the queue (sealed into batches of at most
    /// 256 KiB, as the session does), then the link drains for one tick.
    fn carry(&mut self, packets: &[McpePacket]) {
        let mut added = 0;
        let mut start = 0;
        let mut pending = 0;
        for (index, packet) in packets.iter().enumerate() {
            pending += approximate_size(packet);
            if pending >= 256 * 1024 || index + 1 == packets.len() {
                self.scratch.clear();
                self.codec
                    .encode_batch_multi_into(&packets[start..=index], &mut self.scratch)
                    .expect("encode batch");
                added += self.scratch.len();
                start = index + 1;
                pending = 0;
            }
        }
        let queued = self.queued.load(Ordering::Relaxed) + added;
        self.peak = self.peak.max(queued);
        self.queued.store(
            queued.saturating_sub(self.drain_per_tick),
            Ordering::Relaxed,
        );
    }
}

fn approximate_size(packet: &McpePacket) -> usize {
    match &packet.data {
        McpePacketData::LevelChunkPacket(chunk) => 32 + chunk.serialized_chunk_data.len(),
        McpePacketData::SubChunkPacket(answer) => answer
            .sub_chunk_data
            .iter()
            .map(|entry| 600 + entry.serialized_sub_chunk.as_ref().map_or(0, |p| p.len()))
            .sum(),
        _ => 64,
    }
}

/// A headless player that loads terrain like a client.
pub struct SimPlayer {
    pub session_id: u64,
    pub entity: Entity,
    rx: tokio::sync::mpsc::Receiver<McpePacket>,
    pub link: SimLink,
    requests: Vec<((i32, i32), i32)>,
    /// Columns whose terrain arrived.
    pub complete: HashSet<(i32, i32)>,
    /// `LevelChunk`s received.
    pub headers: usize,
}

impl SimPlayer {
    pub fn join(
        server: &mut GameServer,
        session_id: u64,
        position: DVec3,
        radius: i32,
        drain_per_tick: usize,
    ) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let entity = server
            .spawn_player(PlayerSpawnData {
                session_id,
                display_name: format!("SimPlayer{session_id}"),
                xuid: None,
                uuid: None,
                runtime_id: session_id as i64,
                position,
                saved_data: None,
                outbound_tx: tx,
                chunk_radius: radius,
            })
            .expect("player spawns");
        let link = SimLink::new(drain_per_tick);
        server.attach_send_queue(entity, link.probe());
        Self {
            session_id,
            entity,
            rx,
            link,
            requests: Vec::new(),
            complete: HashSet::new(),
            headers: 0,
        }
    }

    /// Sends this tick's sub-chunk requests (call before `server.tick()`).
    pub fn send_requests(&mut self, server: &mut GameServer) {
        let mut columns = std::mem::take(&mut self.requests).into_iter().peekable();
        while let Some(&(center, _)) = columns.peek() {
            let mut offsets = Vec::new();
            while let Some(&((x, z), limit)) = columns.peek() {
                let (Ok(dx), Ok(dz)) = (i8::try_from(x - center.0), i8::try_from(z - center.1))
                else {
                    break;
                };
                if offsets.len() + limit as usize > MAX_REQUEST_ENTRIES {
                    break;
                }
                columns.next();
                offsets.extend((0..limit).map(|dy| SubChunkPacketPayloadSubChunkPosOffset {
                    subchunk_offset_x: dx,
                    subchunk_offset_y: dy as i8,
                    subchunk_offset_z: dz,
                }));
            }
            let request = McpePacket::from(SubChunkRequestPacket {
                dimension_type: DimensionType { value: 0 },
                sub_chunk_position_offset_list: offsets,
                center_pos: SubChunkPos {
                    subchunk_position_x: center.0,
                    subchunk_position_y: LOWEST_SUBCHUNK,
                    subchunk_position_z: center.1,
                },
            });
            let packet = jolyne::raw::RawPacket::from_packet(&request)
                .and_then(jolyne::raw::RawPacket::decode_borrowed)
                .expect("request decodes");
            server.route_packet(self.session_id, McpePacketArgs, packet);
        }
    }

    /// Reads what the server sent this tick (call after `server.tick()`).
    pub fn receive(&mut self) {
        let mut packets = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            match &packet.data {
                McpePacketData::LevelChunkPacket(chunk) => {
                    self.headers += 1;
                    let column = (chunk.chunk_position.x, chunk.chunk_position.z);
                    let limit = chunk.client_request_sub_chunk_limit.unwrap_or(0);
                    if limit <= 0 {
                        self.complete.insert(column);
                    } else {
                        self.requests
                            .push((column, limit.min(SUBCHUNKS_PER_COLUMN)));
                    }
                }
                McpePacketData::SubChunkPacket(answer) => {
                    let center = &answer.center_pos;
                    for entry in &answer.sub_chunk_data {
                        self.complete.insert((
                            center.subchunk_position_x
                                + i32::from(entry.sub_chunk_pos_offset.subchunk_offset_x),
                            center.subchunk_position_z
                                + i32::from(entry.sub_chunk_pos_offset.subchunk_offset_z),
                        ));
                    }
                }
                _ => {}
            }
            packets.push(packet);
        }
        self.link.carry(&packets);
    }

    /// Columns of the view around block position `at` (the disk of radius + 1
    /// chunks the server streams) not complete yet.
    pub fn missing(&self, at: DVec3, radius: i32) -> usize {
        let center = ((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4);
        let reach = radius + 1;
        (-reach..=reach)
            .flat_map(|dx| (-reach..=reach).map(move |dz| (dx, dz)))
            .filter(|&(dx, dz)| dx * dx + dz * dz <= reach * reach)
            .filter(|&(dx, dz)| !self.complete.contains(&(center.0 + dx, center.1 + dz)))
            .count()
    }

    /// Forgets the terrain received so far, like a client whose view moved
    /// away from it.
    pub fn forget_terrain(&mut self) {
        self.complete.clear();
    }
}
