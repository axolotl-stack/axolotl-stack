//! What ordered packet application would cost: tick time with many players.
//!
//! Ignored by default; run in release:
//!
//! ```sh
//! cargo test -p unastar --release --lib packet_order_bench -- --ignored --nocapture
//! ```
//!
//! Every tick each player sends one `PlayerAuthInput` (walking) and one `MobEquipment`
//! (hotbar switch), routed like the network task routes them. Reported per player
//! count: the median tick (routing included) and the median packet stage
//! (`PacketApplySet`), under three schedules:
//!
//! * `today`: the multi-threaded executor and per-domain queues;
//! * `serial`: the whole schedule on the single-threaded executor, an upper bound on
//!   running the packet stage in one exclusive system;
//! * `model`: as `today`, with every packet first passed through the packet-order model
//!   ([`super::PlayerQueue`]) and released in its order, which is the
//!   bookkeeping an ordered dispatcher adds.

use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use bytes::BytesMut;
use glam::DVec3;
use jolyne::valentine::{
    ActorRuntimeId, BorrowedMcpePacket, EnumsClientPlayMode, EnumsInputMode,
    EnumsNewInteractionModel, EnumsPlayerAuthInputPacketPayloadInputData as Flag, McpePacket,
    McpePacketArgs, McpePacketData, MobEquipmentPacket, PlayerAuthInputPacket, PlayerInputTick,
    Vec2, Vec3,
};

use super::{Arrived, PlayerQueue, timing};
use crate::ecs::PacketApplySet;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

const WARMUP_TICKS: usize = 40;
const MEASURED_TICKS: usize = 200;

/// Start and end of this tick's packet stage.
#[derive(Resource, Default)]
struct StageClock {
    start: Option<Instant>,
    spent: Duration,
}

fn stage_start(mut clock: ResMut<StageClock>) {
    clock.start = Some(Instant::now());
}

fn stage_end(mut clock: ResMut<StageClock>) {
    if let Some(start) = clock.start.take() {
        clock.spent = start.elapsed();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Today,
    Serial,
    Model,
}

struct Players {
    server: GameServer,
    sessions: Vec<(u64, glam::DVec3)>,
    _receivers: Vec<tokio::sync::mpsc::Receiver<McpePacket>>,
}

fn players(count: usize, mode: Mode) -> Players {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(4),
        max_players: count.max(1) as u32,
        ..ServerConfig::default()
    });
    let mut sessions = Vec::new();
    let mut receivers = Vec::new();
    for i in 0..count {
        let session = 1000 + i as u64;
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        // A grid of players two blocks apart on the platform.
        let feet = DVec3::new(
            (i % 16) as f64 * 2.0 + 0.5,
            -60.0,
            (i / 16) as f64 * 2.0 + 0.5,
        );
        server
            .spawn_player(PlayerSpawnData {
                session_id: session,
                display_name: format!("Bench{i}"),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{session:012}")),
                runtime_id: session as i64,
                position: feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("bench player spawns");
        sessions.push((session, feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT));
        receivers.push(rx);
    }
    let world = server.ecs.world_mut();
    world.init_resource::<StageClock>();
    let schedule = server.ecs.schedule_mut();
    schedule.add_systems(stage_start.before(PacketApplySet));
    schedule.add_systems(
        stage_end
            .after(PacketApplySet)
            .before(crate::ecs::PhysicsSet),
    );
    if mode == Mode::Serial {
        schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    }
    Players {
        server,
        sessions,
        _receivers: receivers,
    }
}

fn input(position: DVec3, tick: u64) -> McpePacket {
    McpePacket::from(PlayerAuthInputPacket {
        player_rotation: Vec2 { x: 0.0, y: -90.0 },
        player_head_rotation: -90.0,
        position: Vec3 {
            x: position.x as f32,
            y: position.y as f32,
            z: position.z as f32,
        },
        move_vector: Vec2 { x: 0.0, y: 0.0 },
        input_data: vec![Flag::Verticalcollision],
        input_mode: EnumsInputMode::Mouse,
        play_mode: EnumsClientPlayMode::Normal,
        new_interaction_model: EnumsNewInteractionModel::Crosshair,
        client_tick: PlayerInputTick { inputtick: tick },
        ..Default::default()
    })
}

fn hotbar(runtime_id: u64, slot: u8) -> McpePacket {
    McpePacket::from(MobEquipmentPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        slot,
        selected_slot: slot,
        container_id: 0,
        ..Default::default()
    })
}

fn borrowed(packet: &McpePacketData) -> BorrowedMcpePacket {
    let mut buf = BytesMut::new();
    packet.encode_inner(&mut buf, 0, 0).unwrap();
    BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Median tick and packet stage, in microseconds.
fn measure(count: usize, mode: Mode) -> (f64, f64) {
    let mut bench = players(count, mode);
    let mut queues: Vec<PlayerQueue<McpePacketData>> =
        (0..count).map(|_| PlayerQueue::default()).collect();
    let mut ticks = Vec::new();
    let mut stages = Vec::new();
    for tick in 0..(WARMUP_TICKS + MEASURED_TICKS) as u64 {
        let packets: Vec<(u64, McpePacketData)> = bench
            .sessions
            .iter()
            .flat_map(|&(session, position)| {
                [
                    (session, input(position, tick).data),
                    (session, hotbar(session, (tick % 9) as u8).data),
                ]
            })
            .collect();
        let start = Instant::now();
        if mode == Mode::Model {
            // Network pass: on-arrival packets go straight through, inputs wait for
            // the player pass; both then reach the existing queues in model order.
            let mut released = Vec::with_capacity(packets.len());
            for (session, packet) in packets {
                let index = (session - 1000) as usize;
                let timing = timing(packet.packet_id(), false);
                if let Arrived::Apply(packet) = queues[index].arrive(timing, packet) {
                    released.push((session, packet));
                }
            }
            for (index, queue) in queues.iter_mut().enumerate() {
                for input in queue.drain_inputs() {
                    released.push((1000 + index as u64, input.input));
                }
            }
            for (session, packet) in released {
                bench
                    .server
                    .route_packet(session, McpePacketArgs, borrowed(&packet));
            }
        } else {
            for (session, packet) in packets {
                bench
                    .server
                    .route_packet(session, McpePacketArgs, borrowed(&packet));
            }
        }
        bench.server.tick();
        let elapsed = start.elapsed();
        if tick as usize >= WARMUP_TICKS {
            ticks.push(elapsed);
            stages.push(bench.server.ecs.world().resource::<StageClock>().spent);
        }
    }
    (
        median(ticks).as_secs_f64() * 1e6,
        median(stages).as_secs_f64() * 1e6,
    )
}

#[test]
#[ignore = "benchmark: run in release with --ignored --nocapture"]
fn packet_order_bench() {
    println!("players mode   tick_us  packet_stage_us");
    for count in [10, 50, 200] {
        for mode in [Mode::Today, Mode::Serial, Mode::Model] {
            let (tick, stage) = measure(count, mode);
            println!(
                "{count:7} {:6} {tick:8.0} {stage:16.0}",
                format!("{mode:?}").to_lowercase()
            );
        }
    }
}
