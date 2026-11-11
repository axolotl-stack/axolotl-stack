//! Movement replication against BDS 1.26.51
//! (`testdata/bds-1.26.51-replication.json`: `tools/bds-sweep/sweep.py
//! knockback --items cadence / cadence2 / cadence3 --observer
//! --client-capture` runs distilled by `replication_fixture.py`).
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
    Rotation, RuntimeEntityId, RuntimeId, Velocity,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::combat::{Hit, hurt_mob, player_attack};
use crate::entity::mob::components::{BandwidthOptimization, BandwidthValues, KnockbackPower};
use crate::entity::mob::net::network::{
    MobNetState, Movement, record_spawn_state, replicate_movement, rotation_byte,
};
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
/// each compared up to a sample (all of it). The arrows stop where they
/// enter the floor, on the ground with motion 0, in the tick they hit it.
/// The dispensed projectiles start from their `AddActor` (position,
/// velocity) and the rotation of the first sample; the orb from its
/// `AddActor` velocity.
const REPLAYED: [(&str, usize); 16] = [
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
    ("arrow|fall-32|natural", usize::MAX),
    ("xp_orb|fall-32|natural", usize::MAX),
    ("arrow|wall|natural", usize::MAX),
    ("arrow|floor|natural", usize::MAX),
    ("arrow|floor-2|natural", usize::MAX),
    ("snowball|wall|natural", usize::MAX),
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
    /// Every `AddActor` of the actor, to any viewer.
    adds: Vec<jolyne::valentine::AddActorPacket>,
    /// `MotionPredictionHints` packets any viewer got.
    hints: usize,
}

fn replay(key: &str) -> Replay {
    replay_as(key, None)
}

/// [`replay`] with `actor` spawned in place of the trial's actor.
fn replay_as(key: &str, actor: Option<&str>) -> Replay {
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
    // The block boxes of the setup (a dispenser itself is never in a
    // projectile's way).
    for command in trial["spec"]["setup"].as_array().into_iter().flatten() {
        let command = command.as_str().unwrap();
        if command.starts_with("fill") {
            let (from, to, block) = fill_command(command);
            fill(world, from, to, block);
        }
    }

    let at = &trial["spec"]["at"];
    let spawn = vec3(at);
    let yaw = at[3].as_f64().unwrap() as f32;
    let identifier = actor.unwrap_or_else(|| trial["actor"].as_str().unwrap());
    // What the first viewer's `AddActor` showed.
    let added = &trial["viewers"]["player"]["wire"][0]["add"];
    let actor = if identifier == "minecraft:xp_orb" {
        let orb = crate::entity::xp_orb::spawn_orb(world, spawn, 1);
        let velocity = vec3(&added["vel"]);
        world.get_mut::<Velocity>(orb).unwrap().0 = velocity;
        let rotation = *world.get::<Rotation>(orb).unwrap();
        world
            .entity_mut(orb)
            .insert(MobNetState::spawned(spawn, velocity, rotation, true).motion_unsent());
        orb
    } else if trial["spec"].get("spawn_commands").is_some() {
        // Shot by a dispenser: as it was when BDS added it.
        let first = &trial["samples"][0];
        let (pitch, yaw) = (
            first[4].as_f64().unwrap() as f32,
            first[5].as_f64().unwrap() as f32,
        );
        let at = vec3(&added["pos"]);
        let actor = spawn_mob(world, identifier, at, yaw, SpawnCause::Spawned).unwrap();
        world.get_mut::<Velocity>(actor).unwrap().0 = vec3(&added["vel"]);
        world.get_mut::<Rotation>(actor).unwrap().pitch = pitch;
        record_spawn_state(world, actor, Some(false));
        actor
    } else {
        loop {
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
        }
    };
    world.entity_mut(actor).remove::<MobBrain>();
    let runtime_id = world.get::<RuntimeId>(actor).unwrap().0 as u64;

    let samples = trial["samples"].as_array().unwrap().len();
    let mut adds = Vec::new();
    let mut hints = 0;
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
                    McpePacketData::MotionPredictionHintsPacket(_) => hints += 1,
                    McpePacketData::AddActorPacket(add)
                        if add.target_runtime_id.actor_runtime_id == runtime_id =>
                    {
                        adds.push((*add).clone());
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
    Replay { wire, adds, hints }
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
            assert!(bds.len() >= 2, "{key} {viewer}: {} packets", bds.len());
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

/// The frozen-cow falls 40, 56 and 72 blocks from the viewers.
const COW_FALLS: [&str; 3] = [
    "cow|fall-40|frozen",
    "cow|fall-56|frozen",
    "cow|fall-72|frozen",
];

/// The spawnable mobs whose definitions carry the same empty
/// `conditional_bandwidth_optimization` as the cow and that fall like it (the
/// chicken glides, so its falls differ), with the cow falls each can replay:
/// a zoglin farther than 54 blocks from every player despawns at once.
const EMPTY_COMPONENT_MOBS: [(&str, &[&str]); 9] = [
    ("minecraft:pig", &COW_FALLS),
    ("minecraft:sheep", &COW_FALLS),
    ("minecraft:zombie", &COW_FALLS),
    ("minecraft:husk", &COW_FALLS),
    ("minecraft:zombie_villager_v2", &COW_FALLS),
    ("minecraft:zombie_pigman", &COW_FALLS),
    ("minecraft:wither_skeleton", &COW_FALLS),
    ("minecraft:endermite", &COW_FALLS),
    ("minecraft:zoglin", &["cow|fall-40|frozen"]),
];

/// Each of those mobs, frozen in the BDS frozen-cow falls, sends every
/// viewer exactly the packets BDS sent about the cow: the band comes from
/// the definition's component, and theirs is the cow's.
#[test]
fn mobs_with_the_cows_empty_component_replay_its_bds_cadence() {
    let cow = bandwidth_of("minecraft:cow").unwrap();
    for (identifier, falls) in EMPTY_COMPONENT_MOBS {
        assert_eq!(
            bandwidth_of(identifier).as_ref(),
            Some(&cow),
            "{identifier}: not the cow's component"
        );
        for key in falls {
            let trial = &fixture()["trials"][key];
            let replay = replay_as(key, Some(identifier));
            for (viewer, ours) in &replay.wire {
                let bds = expected(trial, viewer, false, usize::MAX);
                let ours: Vec<&Wire> = ours
                    .iter()
                    .filter(|event| compared(event, false, usize::MAX))
                    .collect();
                assert_eq!(ours.len(), bds.len(), "{identifier} {key} {viewer}");
                for (ours, bds) in ours.iter().zip(&bds) {
                    assert!(
                        ours.matches(bds, false),
                        "{identifier} {key} {viewer}: {ours:?}, BDS {bds:?}"
                    );
                }
            }
        }
    }
}

/// The spawnable actors no BDS trial replays compile the values their
/// definitions author: the chicken has the cow's empty component (it glides,
/// so the cow's falls do not replay it), the bottle o' enchanting 5 / 80.
#[test]
fn spawnable_actors_without_a_trial_compile_their_bandwidth_values() {
    assert_eq!(
        bandwidth_of("minecraft:chicken"),
        bandwidth_of("minecraft:cow")
    );
    let bottle = bandwidth_of("minecraft:xp_bottle").unwrap();
    assert!(bottle.conditional.is_empty());
    assert_eq!(bottle.default.max_dropped_ticks, 5);
    assert_eq!(bottle.default.max_optimized_distance, 80.0);
    assert_eq!(bottle.default.dropped_ticks(48.0), 3);
}

/// A vanilla actor's `conditional_bandwidth_optimization` (`None` without
/// the component).
fn bandwidth_of(identifier: &str) -> Option<BandwidthOptimization> {
    let definition = unastar_data::actor_definitions::get(identifier).unwrap();
    let body = definition
        .components
        .get("minecraft:conditional_bandwidth_optimization")?;
    let body: unastar_data::actor_components::body::ConditionalBandwidthOptimization =
        serde_json::from_value(body.clone()).unwrap();
    Some(BandwidthOptimization::compile(body).unwrap())
}

/// The values in force at `sample`: the conditional ones (vehicles: 0 / 0)
/// while the actor moved in the tick before.
fn values_at(
    bandwidth: &BandwidthOptimization,
    samples: &[Value],
    sample: usize,
) -> BandwidthValues {
    let moved = sample >= 2 && vec3(&samples[sample - 1]) != vec3(&samples[sample - 2]);
    bandwidth.values(moved)
}

/// Every actor of the fixture, also those this server cannot spawn yet: the
/// `ticks` of each position update BDS sent is the band its own definition
/// gives for the viewer's distance (eye to the actor), with a vehicle's
/// `is_moving` values (0 / 0) while it moved in the tick before, or 0 when
/// the update changed its ground contact; without the component every update
/// has `ticks` 0 and goes out every tick.
#[test]
fn bds_ticks_are_the_band_of_each_actors_own_bandwidth_values() {
    let mut banded = 0;
    for (key, trial) in fixture()["trials"].as_object().unwrap() {
        let identifier = trial["actor"].as_str().unwrap();
        let bandwidth = bandwidth_of(identifier);
        let offset = DVec3::Y * trial["wire_offset"].as_f64().unwrap_or(0.0);
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
                    let distance = eye(sample).distance(vec3(&samples[sample]) + offset);
                    let band = bandwidth.as_ref().map_or(0, |bandwidth| {
                        values_at(bandwidth, samples, sample).dropped_ticks(distance)
                    });
                    assert!(
                        ticks == band || (ticks == 0 && changed),
                        "{key} {viewer} sample {sample}: ticks {ticks}, band {band} at {distance}"
                    );
                    banded += usize::from(band > 0);
                }
                if bandwidth.is_none()
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
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    // A viewer next to the summon, in its send range.
    let viewer = world
        .spawn((
            Player,
            Position(feet + DVec3::new(3.0, 1.62, 0.0)),
            PlayerSession::new(1, "viewer".into(), None, None, tx),
        ))
        .id();
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

/// A position update as a viewer saw it: sample, position axes, ground
/// contact and `ticks`.
type DeltaSeen = (usize, [Option<f64>; 3], bool, u64);

/// The minecart and the boat (not spawnable here) through the movement
/// replication alone: fed the positions and ground contact BDS sampled, each
/// viewer gets BDS's updates. The first update after rest uses their default
/// values (20 ticks over 60 blocks: `ticks` 10 for the player 32 blocks away,
/// 17 for the observer at 51), every later one while they keep falling goes
/// out every tick with `ticks` 0 (`is_moving`: 0 / 0), and a fall that
/// leaves the ground goes at once.
#[test]
fn vehicles_update_every_tick_while_they_move() {
    for key in [
        "minecart|fall-32|natural",
        "minecart|rest-fall|natural",
        "boat|rest-fall|natural",
    ] {
        let trial = &fixture()["trials"][key];
        let bandwidth = bandwidth_of(trial["actor"].as_str().unwrap()).unwrap();
        let samples = trial["samples"].as_array().unwrap();
        let offset = DVec3::Y * trial["wire_offset"].as_f64().unwrap();
        let wire_at = |sample: usize| vec3(&samples[sample]) + offset;
        let ground_at = |sample: usize| samples[sample][3].as_u64().unwrap() == 1;
        let rotation = |sample: usize| Rotation {
            pitch: samples[sample][4].as_f64().unwrap() as f32,
            yaw: samples[sample][5].as_f64().unwrap() as f32,
            head_yaw: 0.0,
        };
        let mut world = World::new();
        let viewers: Vec<(&str, Entity, DVec3)> = VIEWERS
            .into_iter()
            .filter_map(|name| {
                let seen = &trial["viewers"][name];
                let at = &seen["positions"][0];
                let eye = DVec3::new(
                    at[1].as_f64()?,
                    at[2].as_f64()? + crate::entity::PLAYER_EYE_HEIGHT,
                    at[3].as_f64()?,
                );
                Some((name, world.spawn_empty().id(), eye))
            })
            .collect();
        let movers: Vec<Entity> = viewers.iter().map(|(_, entity, _)| *entity).collect();
        let added = &trial["viewers"]["player"]["wire"][0]["add"];
        let mut net = MobNetState::spawned(
            vec3(&added["pos"]),
            vec3(&added["vel"]),
            rotation(0),
            ground_at(0),
        );
        let mut ours: Vec<(&str, DeltaSeen)> = Vec::new();
        for sample in 1..samples.len() {
            let movement = Movement::new(
                1,
                wire_at(sample),
                &rotation(sample),
                ground_at(sample),
                wire_at(sample) - wire_at(sample - 1),
            );
            replicate_movement(
                &mut net,
                &movement,
                Some(&bandwidth),
                &movers,
                sample as u64,
                |viewer| {
                    viewers
                        .iter()
                        .find(|(_, entity, _)| *entity == viewer)
                        .map(|(_, _, eye)| *eye)
                },
                |viewer, packet| {
                    if let McpePacketData::MoveActorDeltaPacket(delta) = &packet.data {
                        let data = &delta.move_data;
                        let name = viewers.iter().find(|(_, e, _)| *e == viewer).unwrap().0;
                        let axes = [
                            data.new_position_x,
                            data.new_position_y,
                            data.new_position_z,
                        ]
                        .map(|axis| axis.map(f64::from));
                        ours.push((name, (sample, axes, data.is_on_ground, data.ticks)));
                    }
                },
            );
        }
        for (name, _, _) in &viewers {
            let bds: Vec<DeltaSeen> = trial["viewers"][name]["wire"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event.get("delta").is_some())
                .map(|event| {
                    (
                        event["sample"].as_u64().unwrap() as usize,
                        std::array::from_fn::<_, 3, _>(|axis| event["delta"][axis].as_f64()),
                        event["ground"].as_bool().unwrap(),
                        event["ticks"].as_u64().unwrap(),
                    )
                })
                .collect();
            let mine: Vec<DeltaSeen> = ours
                .iter()
                .filter(|(viewer, _)| viewer == name)
                .map(|(_, seen)| *seen)
                .collect();
            assert_eq!(mine.len(), bds.len(), "{key} {name}: {mine:?}\nBDS {bds:?}");
            for (ours, bds) in mine.iter().zip(&bds) {
                let same_axes = ours.1.iter().zip(&bds.1).all(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => (a - b).abs() < TOLERANCE,
                    (None, None) => true,
                    _ => false,
                });
                assert!(
                    ours.0 == bds.0 && same_axes && ours.2 == bds.2 && ours.3 == bds.3,
                    "{key} {name}: {ours:?}, BDS {bds:?}"
                );
            }
        }
    }
}

/// A projectile's `AddActor` carries its pitch and yaw cut to whole
/// rotation bytes and head and body yaw 0 (BDS 1.26.51: a dispensed arrow at
/// pitch -5.768 and yaw 89.885 was added with -5.625 and 88.59375).
#[test]
fn a_projectile_add_actor_has_its_rotation_in_bytes_and_no_head_or_body_yaw() {
    let replay = replay("arrow|wall|natural");
    let trial = &fixture()["trials"]["arrow|wall|natural"];
    let first = &trial["samples"][0];
    let (pitch, yaw) = (
        first[4].as_f64().unwrap() as f32,
        first[5].as_f64().unwrap() as f32,
    );
    assert!(!replay.adds.is_empty());
    for add in &replay.adds {
        assert_eq!(
            [
                add.rotation.x,
                add.rotation.y,
                add.y_head_rotation,
                add.y_body_rotation
            ],
            [-5.625, 88.59375, 0.0, 0.0]
        );
        assert_eq!(rotation_byte(add.rotation.y), rotation_byte(yaw));
        assert_eq!(rotation_byte(add.rotation.x), rotation_byte(pitch));
    }
}

/// BDS 1.26.51 sent no `MotionPredictionHints` to either viewer in any
/// trial, although most of their actors have `use_motion_prediction_hints`
/// (minecarts, the boat, arrows, the orb, the enderman, snowballs, the egg),
/// and this server sends none in their replays.
#[test]
fn no_motion_prediction_hints_are_sent() {
    let mut with_hints = 0;
    for (key, trial) in fixture()["trials"].as_object().unwrap() {
        for viewer in VIEWERS {
            let Some(seen) = trial["viewers"].get(viewer) else {
                continue;
            };
            assert!(
                seen["wire"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|event| event.get("hint").is_none()),
                "{key} {viewer}"
            );
        }
        if let Some(bandwidth) = bandwidth_of(trial["actor"].as_str().unwrap())
            && bandwidth.default.use_motion_prediction_hints
        {
            with_hints += 1;
        }
    }
    assert!(with_hints >= 10, "{with_hints} trials of actors with hints");
    for (key, _) in REPLAYED {
        assert_eq!(replay(key).hints, 0, "{key}");
    }
}
