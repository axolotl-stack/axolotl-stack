//! Movement replication against BDS 1.26.51
//! (`testdata/bds-1.26.51-replication.json`: `tools/bds-sweep/sweep.py
//! knockback --items cadence / cadence2 --observer --client-capture` runs
//! distilled by `replication_fixture.py`).
//!
//! Each trial is replayed tick by tick with the same actor, the same two
//! viewers where BDS showed them the actor (the sweep player and a second
//! client, both creative, standing still), the same blocks and the same hits.
//! Then every viewer's `AddActor`, position update and motion packet about
//! the actor must equal what that viewer got from BDS: same tick, same axes
//! and values, same ground flag and the same `ticks`. Frozen cows still turn
//! their heads on BDS and not here, so a cow's rotation bytes are not
//! compared; a projectile's are.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::EntityRandom;
use crate::entity::components::{
    Experience, GameMode, Health, HeldSlot, MainInventory, Player, PlayerSession, Position,
    Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::combat::{Hit, hurt_mob, player_attack};
use crate::entity::mob::components::{BandwidthOptimization, BandwidthValues, KnockbackPower};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

/// BDS moves actors in `f32`: after a 60-block fall a replayed position is
/// up to 3e-5 from BDS's (the physics replays of `combat::bds_knockback`
/// hold 2e-5 over smaller moves).
const TOLERANCE: f64 = 1.0e-4;

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-replication.json")).unwrap()
    })
}

/// The trials whose actor this server spawns (every BDS cow was an adult),
/// each compared up to a sample: the arrow only until it hits the floor,
/// because BDS stops it on the grass top that very tick (on the ground,
/// motion 0) and this server's stuck-arrow landing does not yet.
const REPLAYED: [(&str, usize); 11] = [
    ("cow|fall-40|frozen", usize::MAX),
    ("cow|fall-56|frozen", usize::MAX),
    ("cow|fall-72|frozen", usize::MAX),
    ("cow|cap|frozen", usize::MAX),
    ("cow|pause-40|frozen", usize::MAX),
    ("cow|wall|frozen", usize::MAX),
    ("cow|wall-far|frozen", usize::MAX),
    ("snowball|fall-32|natural", usize::MAX),
    ("snowball|fall-56|natural", usize::MAX),
    ("egg|fall-32|natural", usize::MAX),
    ("arrow|fall-32|natural", 31),
];

const VIEWERS: [&str; 2] = ["player", "observer"];

fn vec3(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

/// One packet a viewer got about the actor, tied to the tick (sample) it
/// reports.
#[derive(Debug, Clone)]
enum Wire {
    Add {
        position: DVec3,
        velocity: DVec3,
    },
    Move {
        sample: usize,
        position: [Option<f64>; 3],
        rotation: [Option<u8>; 3],
        on_ground: bool,
        ticks: u64,
    },
    Motion {
        sample: usize,
        motion: DVec3,
    },
}

impl Wire {
    fn from_fixture(event: &Value) -> Self {
        let sample = event["sample"].as_u64().unwrap() as usize;
        if let Some(add) = event.get("add") {
            return Wire::Add {
                position: vec3(&add["pos"]),
                velocity: vec3(&add["vel"]),
            };
        }
        match event.get("delta") {
            Some(delta) => Wire::Move {
                sample,
                position: std::array::from_fn(|axis| delta[axis].as_f64()),
                rotation: std::array::from_fn(|axis| {
                    event["rot"][axis].as_u64().map(|byte| byte as u8)
                }),
                on_ground: event["ground"].as_bool().unwrap(),
                ticks: event["ticks"].as_u64().unwrap(),
            },
            None => Wire::Motion {
                sample,
                motion: vec3(&event["motion"]),
            },
        }
    }

    fn carries_position(&self) -> bool {
        matches!(self, Wire::Move { position, .. } if position.iter().any(Option::is_some))
    }

    fn matches(&self, other: &Wire, rotation: bool) -> bool {
        let close = |a: DVec3, b: DVec3| (a - b).abs().max_element() < TOLERANCE;
        match (self, other) {
            (
                Wire::Add {
                    position: pa,
                    velocity: va,
                },
                Wire::Add {
                    position: pb,
                    velocity: vb,
                },
            ) => close(*pa, *pb) && close(*va, *vb),
            (
                Wire::Move {
                    sample: a,
                    position: pa,
                    rotation: ra,
                    on_ground: ga,
                    ticks: ta,
                },
                Wire::Move {
                    sample: b,
                    position: pb,
                    rotation: rb,
                    on_ground: gb,
                    ticks: tb,
                },
            ) => {
                a == b
                    && ga == gb
                    && ta == tb
                    && (!rotation || ra == rb)
                    && pa.iter().zip(pb).all(|(x, y)| match (x, y) {
                        (Some(x), Some(y)) => (x - y).abs() < TOLERANCE,
                        (None, None) => true,
                        _ => false,
                    })
            }
            (
                Wire::Motion {
                    sample: a,
                    motion: ma,
                },
                Wire::Motion {
                    sample: b,
                    motion: mb,
                },
            ) => a == b && close(*ma, *mb),
            _ => false,
        }
    }
}

/// Fill the box `from..=to` (block coordinates) of the loaded chunks.
fn fill(world: &mut World, from: [i32; 3], to: [i32; 3], block: u32) {
    for x in from[0]..=to[0] {
        for y in from[1]..=to[1] {
            for z in from[2]..=to[2] {
                let (cx, cz) = crate::world::ecs::world_to_chunk_coords(x, z);
                let chunk = world
                    .resource::<ChunkManager>()
                    .get_by_coords(cx, cz)
                    .unwrap();
                world.get_mut::<ChunkData>(chunk).unwrap().inner.set_block(
                    x.rem_euclid(16) as u8,
                    y as i16,
                    z.rem_euclid(16) as u8,
                    block,
                );
            }
        }
    }
}

/// The `fill` commands of a trial's setup and actions, as block boxes.
fn fill_command(command: &str) -> ([i32; 3], [i32; 3], u32) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(parts[0], "fill", "{command}");
    let n = |i: usize| parts[i].parse::<i32>().unwrap();
    let block = match parts[7] {
        "stone" => *blocks::STONE,
        "air" => *blocks::AIR,
        other => panic!("unexpected block {other}"),
    };
    ([n(1), n(2), n(3)], [n(4), n(5), n(6)], block)
}

/// What each viewer got in the replay of one trial.
struct Replay {
    wire: Vec<(&'static str, Vec<Wire>)>,
}

fn replay(key: &str) -> Replay {
    let trial = &fixture()["trials"][key];
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(7));
    *world.resource_mut::<Difficulty>() = Difficulty::Easy;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    crate::world::clock::set_time(world, 18000);

    // The viewers BDS showed the actor to, where they stood at the spawn.
    let mut viewers = Vec::new();
    for (index, name) in VIEWERS.into_iter().enumerate() {
        let seen = &trial["viewers"][name];
        if seen.is_null() || seen["wire"].as_array().unwrap().is_empty() {
            continue;
        }
        let at = &seen["positions"][0];
        let feet = DVec3::new(
            at[1].as_f64().unwrap(),
            at[2].as_f64().unwrap(),
            at[3].as_f64().unwrap(),
        );
        let (tx, rx) = mpsc::channel::<McpePacket>(1 << 16);
        let runtime = index as i64 + 1;
        let entity = world
            .spawn((
                Player,
                Position(feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
                Rotation::default(),
                GameMode::Creative,
                MainInventory::default(),
                HeldSlot(0),
                Health::default(),
                Experience::default(),
                RuntimeEntityId(runtime),
                PlayerSession::new(runtime as u64, name.into(), None, None, tx),
            ))
            .id();
        viewers.push((name, entity, rx));
    }
    // A grass floor at y = -61 (the BDS superflat) wide enough for every
    // trial, every chunk viewed by every viewer.
    for cx in -4..=5 {
        for cz in -4..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, -61, z, *blocks::GRASS_BLOCK);
                }
            }
            let mut chunk_viewers = ChunkViewers::default();
            for (_, viewer, _) in &viewers {
                chunk_viewers.insert(*viewer);
            }
            let chunk = world.spawn((ChunkData::new(column), chunk_viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }
    for command in trial["spec"]["setup"].as_array().into_iter().flatten() {
        let (from, to, block) = fill_command(command.as_str().unwrap());
        fill(world, from, to, block);
    }

    let at = &trial["spec"]["at"];
    let spawn = vec3(at);
    let yaw = at[3].as_f64().unwrap() as f32;
    let identifier = trial["actor"].as_str().unwrap();
    let actor = loop {
        let actor = spawn_mob(world, identifier, spawn, yaw, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        let baby = world
            .get::<ActorFlags>(actor)
            .and_then(|flags| flags.get(actor_flags::BABY).ok())
            .unwrap_or(false);
        if !baby {
            break actor;
        }
        world.despawn(actor);
    };
    world.entity_mut(actor).remove::<MobBrain>();
    let runtime_id = world.get::<RuntimeId>(actor).unwrap().0 as u64;

    let samples = trial["samples"].as_array().unwrap().len();
    let mut wire: Vec<(&'static str, Vec<Wire>)> = viewers
        .iter()
        .map(|(name, _, _)| (*name, Vec::new()))
        .collect();
    // Sample 0 is the spawn; sample n follows the n-th tick.
    for sample in 1..samples {
        let world = server.ecs.world_mut();
        for hit in trial["hits"].as_array().unwrap() {
            if hit["sample"].as_u64().unwrap() as usize != sample {
                continue;
            }
            let source = vec3(&hit["source"]) + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
            let attacker = viewers[0].1;
            if trial["spec"]["hits"].as_array().unwrap().is_empty() {
                // `/damage … entity_attack entity <player>` from afar.
                hurt_mob(
                    world,
                    actor,
                    Hit {
                        damage: 1.0,
                        cause: ActorDamageCause::EntityAttack,
                        attacker: Some(attacker),
                        by_player: true,
                        knockback_from: Some(source),
                        knockback_power: KnockbackPower::DEFAULT,
                    },
                );
            } else {
                world.get_mut::<Position>(attacker).unwrap().0 = source;
                player_attack(world, attacker, actor, &Default::default());
            }
        }
        // Commands run after a sample is taken; the next tick shows them.
        for action in trial["spec"]["actions"].as_array().into_iter().flatten() {
            if action["t"].as_u64().unwrap() as usize + 1 == sample {
                for command in action["commands"].as_array().unwrap() {
                    let command = command.as_str().unwrap();
                    if command.starts_with("fill") {
                        let (from, to, block) = fill_command(command);
                        fill(world, from, to, block);
                    }
                }
            }
        }
        server.tick();
        for ((_, _, rx), (_, events)) in viewers.iter_mut().zip(wire.iter_mut()) {
            while let Ok(packet) = rx.try_recv() {
                match packet.data {
                    McpePacketData::AddActorPacket(add)
                        if add.target_runtime_id.actor_runtime_id == runtime_id =>
                    {
                        events.push(Wire::Add {
                            position: DVec3::new(
                                add.position.x.into(),
                                add.position.y.into(),
                                add.position.z.into(),
                            ),
                            velocity: DVec3::new(
                                add.velocity.x.into(),
                                add.velocity.y.into(),
                                add.velocity.z.into(),
                            ),
                        });
                    }
                    McpePacketData::MoveActorDeltaPacket(moved)
                        if moved.move_data.actor_runtime_id.actor_runtime_id == runtime_id =>
                    {
                        let data = moved.move_data;
                        events.push(Wire::Move {
                            sample,
                            position: [
                                data.new_position_x,
                                data.new_position_y,
                                data.new_position_z,
                            ]
                            .map(|axis| axis.map(f64::from)),
                            rotation: [data.rotation_x, data.rotation_y, data.rotation_y_head]
                                .map(|byte| byte.map(|byte| byte as u8)),
                            on_ground: data.is_on_ground,
                            ticks: data.ticks,
                        });
                    }
                    McpePacketData::SetActorMotionPacket(motion)
                        if motion.target_runtime_id.actor_runtime_id == runtime_id =>
                    {
                        let motion = motion.motion;
                        events.push(Wire::Motion {
                            sample,
                            motion: DVec3::new(motion.x.into(), motion.y.into(), motion.z.into()),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    Replay { wire }
}

/// BDS's packets for one viewer that the replay must reproduce: the
/// `AddActor`, every position update (for a cow, whose head BDS turned on
/// its own, without the updates that carried only a rotation) and every
/// motion packet.
fn expected(trial: &Value, viewer: &str, projectile: bool, until: usize) -> Vec<Wire> {
    trial["viewers"][viewer]["wire"]
        .as_array()
        .unwrap()
        .iter()
        .map(Wire::from_fixture)
        .filter(|event| compared(event, projectile, until))
        .collect()
}

fn compared(event: &Wire, projectile: bool, until: usize) -> bool {
    let before = match event {
        Wire::Add { .. } => true,
        Wire::Move { sample, .. } | Wire::Motion { sample, .. } => *sample < until,
    };
    before && (projectile || !matches!(event, Wire::Move { .. }) || event.carries_position())
}

#[test]
fn every_viewer_gets_the_bds_position_updates_at_its_own_cadence() {
    for (key, until) in REPLAYED {
        let trial = &fixture()["trials"][key];
        let projectile = !trial["actor"].as_str().unwrap().ends_with(":cow");
        let replay = replay(key);
        for (viewer, ours) in &replay.wire {
            let bds = expected(trial, viewer, projectile, until);
            assert!(bds.len() > 5, "{key} {viewer}: {} packets", bds.len());
            let ours: Vec<&Wire> = ours
                .iter()
                .filter(|event| compared(event, projectile, until))
                .collect();
            assert_eq!(
                ours.len(),
                bds.len(),
                "{key} {viewer}: packets\n ours {ours:#?}\n BDS {bds:#?}"
            );
            for (ours, bds) in ours.iter().zip(&bds) {
                assert!(
                    ours.matches(bds, projectile),
                    "{key} {viewer}: {ours:?}, BDS {bds:?}"
                );
            }
        }
    }
}

/// The bandwidth values a vanilla actor's definition gives it (`None`
/// without the component).
fn bandwidth_of(identifier: &str) -> Option<BandwidthValues> {
    let definition = unastar_data::actor_definitions::get(identifier).unwrap();
    let body = definition
        .components
        .get("minecraft:conditional_bandwidth_optimization")?;
    let body: unastar_data::actor_components::body::ConditionalBandwidthOptimization =
        serde_json::from_value(body.clone()).unwrap();
    Some(BandwidthOptimization::compile(body).unwrap().0)
}

/// Every actor of the fixture, also those this server cannot spawn yet: the
/// `ticks` of each position update BDS sent is the band its own definition
/// gives for the viewer's distance (eye to the actor), or 0 when the update
/// changed its ground contact; without the component every update has
/// `ticks` 0 and goes out every tick. (The minecart's conditional values are
/// rejected: its updates follow `is_moving`.)
#[test]
fn bds_ticks_are_the_band_of_each_actors_own_bandwidth_values() {
    let mut banded = 0;
    for (key, trial) in fixture()["trials"].as_object().unwrap() {
        let identifier = trial["actor"].as_str().unwrap();
        if identifier == "minecraft:minecart" {
            let definition = unastar_data::actor_definitions::get(identifier).unwrap();
            let body = serde_json::from_value(
                definition.components["minecraft:conditional_bandwidth_optimization"].clone(),
            )
            .unwrap();
            assert!(matches!(
                BandwidthOptimization::compile(body),
                Err(super::super::components::bandwidth::BandwidthError::ConditionalValues)
            ));
            continue;
        }
        let values = bandwidth_of(identifier);
        let samples = trial["samples"].as_array().unwrap();
        for viewer in VIEWERS {
            let Some(seen) = trial["viewers"].get(viewer) else {
                continue;
            };
            let positions = seen["positions"].as_array().unwrap();
            let eye = |sample: usize| {
                let at = positions
                    .iter()
                    .rev()
                    .find(|row| row[0].as_u64().unwrap() as usize <= sample)
                    .unwrap();
                DVec3::new(
                    at[1].as_f64().unwrap(),
                    at[2].as_f64().unwrap() + crate::entity::PLAYER_EYE_HEIGHT,
                    at[3].as_f64().unwrap(),
                )
            };
            let mut last: Option<(usize, bool)> = None;
            for event in seen["wire"].as_array().unwrap() {
                if event.get("delta").is_none() {
                    continue;
                }
                let ground = event["ground"].as_bool().unwrap();
                let ticks = event["ticks"].as_u64().unwrap();
                let sample = event["sample"].as_u64().unwrap() as usize;
                let changed = last.is_none_or(|(_, before)| before != ground);
                if event.get("timed").is_none() {
                    let distance = eye(sample).distance(vec3(&samples[sample]));
                    let band = values.map_or(0, |values| values.dropped_ticks(distance));
                    assert!(
                        ticks == band || (ticks == 0 && changed),
                        "{key} {viewer} sample {sample}: ticks {ticks}, band {band} at {distance}"
                    );
                    banded += usize::from(band > 0);
                }
                if values.is_none()
                    && let Some((before, _)) = last
                {
                    assert_eq!(sample, before + 1, "{key} {viewer}: every tick");
                }
                last = Some((sample, ground));
            }
        }
    }
    assert!(banded > 100, "{banded} banded updates checked");
}

/// The `AddActor` a summoned cow's viewer gets in the summon tick shows the
/// cow as it spawned, like the native summon capture, although the cow has
/// already made its first move by the time the packet is sent.
#[test]
fn the_summon_tick_add_actor_shows_the_mob_before_its_first_move() {
    use valentine::bedrock::codec::BedrockCodec as _;
    let bytes =
        include_bytes!("../../../../../bds-extractor/captures/1.26.51/operator/summons/cow.bin");
    let mut input = &bytes[1..];
    let native = jolyne::valentine::AddActorPacket::decode(&mut input, ()).unwrap();
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    let (tx, mut rx) = mpsc::channel::<McpePacket>(1 << 12);
    let viewer = world
        .spawn((
            Player,
            Position(DVec3::new(0.5, 100.0, 0.5)),
            PlayerSession::new(1, "viewer".into(), None, None, tx),
        ))
        .id();
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let (cx, cz) = crate::world::ecs::world_to_chunk_coords(feet.x as i32, feet.z as i32);
    let mut chunk_viewers = ChunkViewers::default();
    chunk_viewers.insert(viewer);
    let chunk = world
        .spawn((ChunkData::new(Chunk::new(cx, cz)), chunk_viewers))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(cx, cz), chunk);
    let cow = spawn_mob(world, "minecraft:cow", feet, 0.0, SpawnCause::Spawned).unwrap();
    server.tick();
    let moved = server.ecs.world().get::<Position>(cow).unwrap().0;
    assert!(moved.y < feet.y, "the cow made its first move");
    let add = std::iter::from_fn(|| rx.try_recv().ok())
        .find_map(|packet| match packet.data {
            McpePacketData::AddActorPacket(add) => Some(add),
            _ => None,
        })
        .unwrap();
    assert_eq!(add.position, native.position);
    assert_eq!(add.velocity, native.velocity);
}
