//! Another player's movement as a client sees it, against BDS 1.26.51
//! (`testdata/bds-1.26.51-player-relay.json`: `tools/bds-sweep/sweep.py walk
//! --observer --client-capture`, distilled by `player_relay_fixture.py`).
//!
//! Each scripted walk (standing still, turning, tilting, walking, stepping
//! up) is replayed input by input: the moving player sends the pitch, yaw
//! and head yaw its BDS client sent, at the position BDS moved it to (a
//! vanilla client predicts that same position), on BDS's superflat floor.
//! Every input must reach the observer as one `MovePlayer` with BDS's
//! position, pitch, yaw, head yaw, mode, ground contact and tick.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{PlayerInputTick, Vec2, Vec3};
use jolyne::valentine::{McpePacket, McpePacketData, PlayerAuthInputPacket};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::entity::components::{
    ActorViewers, GameMode, Player, PlayerInput, PlayerName, PlayerSession, PlayerState,
    PlayerUuid, Position, Rotation, RuntimeEntityId,
};
use crate::server::game::packet_queues::{MovementInput, MovementPacketQueue};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition};

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-player-relay.json")).unwrap()
    })
}

fn f32_of(value: &Value) -> f32 {
    value.as_f64().unwrap() as f32
}

fn player(world: &mut World, id: i64, eye: DVec3) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel::<McpePacket>(1 << 16);
    let entity = world
        .spawn((
            (
                Player,
                Position(eye),
                Rotation::default(),
                GameMode::Creative,
                RuntimeEntityId(id),
                PlayerUuid(Uuid::from_u128(id as u128)),
                PlayerName(format!("player{id}")),
            ),
            (
                PlayerSession::new(id as u64, format!("player{id}"), None, None, tx),
                PlayerState::default(),
                PlayerInput::default(),
                ActorViewers::default(),
            ),
        ))
        .id();
    (entity, rx)
}

#[test]
fn every_input_reaches_the_other_player_as_bds_relays_it() {
    let mut compared = 0;
    for item in fixture()["items"].as_array().unwrap() {
        let steps = item["steps"].as_array().unwrap();
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        // BDS's superflat: grass at y -61, the floor at -60.
        for cx in -2..=1 {
            for cz in -2..=1 {
                let mut column = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        column.set_block(x, -61, z, *blocks::GRASS_BLOCK);
                    }
                }
                let chunk = world.spawn(ChunkData::new(column)).id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(ChunkPosition::new(cx, cz), chunk);
            }
        }
        let first = &steps[0]["relay"]["pos"];
        let start = DVec3::new(
            first[0].as_f64().unwrap(),
            first[1].as_f64().unwrap(),
            first[2].as_f64().unwrap(),
        );
        let (mover, _mover_rx) = player(world, 1, start);
        let (_, mut rx) = player(world, 2, start + DVec3::new(0.0, 0.0, 8.0));
        // The observer has the mover in view before the walk.
        server.tick();
        server.tick();
        std::iter::from_fn(|| rx.try_recv().ok()).for_each(drop);
        for (index, step) in steps.iter().enumerate() {
            let (input, bds) = (&step["input"], &step["relay"]);
            let pos = &bds["pos"];
            let packet = PlayerAuthInputPacket {
                position: Vec3 {
                    x: f32_of(&pos[0]),
                    y: f32_of(&pos[1]),
                    z: f32_of(&pos[2]),
                },
                player_rotation: Vec2 {
                    x: f32_of(&input["pitch"]),
                    y: f32_of(&input["yaw"]),
                },
                player_head_rotation: f32_of(&input["head"]),
                client_tick: PlayerInputTick {
                    inputtick: bds["tick"].as_u64().unwrap(),
                },
                ..Default::default()
            };
            server
                .ecs
                .world_mut()
                .resource_mut::<MovementPacketQueue>()
                .0
                .push((mover, MovementInput::AuthInput(Box::new(packet))));
            server.tick();
            let relays: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
                .filter_map(|packet| match packet.data {
                    McpePacketData::MovePlayerPacket(moved)
                        if moved.player_runtime_id.actor_runtime_id == 1 =>
                    {
                        Some(moved)
                    }
                    _ => None,
                })
                .collect();
            let key = item["key"].as_str().unwrap();
            assert_eq!(relays.len(), 1, "{key} input {index}: one MovePlayer");
            let ours = &relays[0];
            let close = |a: f32, b: &Value| (a - f32_of(b)).abs() < 1.0e-3;
            assert!(
                close(ours.position.x, &pos[0])
                    && close(ours.position.y, &pos[1])
                    && close(ours.position.z, &pos[2]),
                "{key} input {index}: position {:?}, BDS {pos}",
                ours.position
            );
            assert!(
                close(ours.rotation.x, &bds["pitch"])
                    && close(ours.rotation.y, &bds["yaw"])
                    && close(ours.y_head_rotation, &bds["head"]),
                "{key} input {index}: rotation {:?} head {}, BDS {bds}",
                ours.rotation,
                ours.y_head_rotation
            );
            assert_eq!(
                ours.on_ground,
                bds["ground"].as_bool().unwrap(),
                "{key} input {index}: ground"
            );
            assert_eq!(ours.tick.inputtick, bds["tick"].as_u64().unwrap());
            assert!(ours.teleport_data.is_none());
            compared += 1;
        }
    }
    assert!(compared > 400, "{compared} relayed inputs compared");
}

/// BDS sends a mob's `AddActor` rotation cut to whole rotation bytes: the
/// cow each walk item spawned at yaw 33.3 came with pitch 0 and yaw, head
/// and body yaw 32.34375.
#[test]
fn a_spawned_mob_add_actor_carries_its_yaw_cut_to_rotation_bytes() {
    let bds = &fixture()["items"][0]["cow_add_actor_rotation"];
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    let (_, mut rx) = player(world, 1, DVec3::new(0.5, 66.0, 0.5));
    let cow = crate::entity::mob::spawn_mob(
        world,
        "minecraft:cow",
        DVec3::new(3.5, 64.0, -3.5),
        33.3,
        crate::entity::mob::SpawnCause::Spawned,
    )
    .unwrap();
    let runtime_id = world
        .get::<crate::entity::components::RuntimeId>(cow)
        .unwrap()
        .0 as u64;
    server.tick();
    let add = std::iter::from_fn(|| rx.try_recv().ok())
        .find_map(|packet| match packet.data {
            McpePacketData::AddActorPacket(add)
                if add.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(add)
            }
            _ => None,
        })
        .expect("the cow is added");
    let ours = [
        add.rotation.x,
        add.rotation.y,
        add.y_head_rotation,
        add.y_body_rotation,
    ];
    for (ours, bds) in ours.into_iter().zip(bds.as_array().unwrap()) {
        assert!((ours - f32_of(bds)).abs() < 1.0e-5, "{ours} vs BDS {bds}");
    }
}
