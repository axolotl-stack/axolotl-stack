//! Knockback and the movement it causes, against BDS 1.26.51
//! (`testdata/bds-1.26.51-knockback.json`: `tools/bds-sweep/sweep.py
//! knockback --client-capture` runs distilled by `knockback_fixture.py`;
//! `testdata/bds-1.26.51-knockback-resistance.json`: the same for the
//! zoglin, hoglin and iron golem, `--items resist`).
//!
//! The frozen trials are replayed tick by tick. The mob spawns where BDS
//! spawned it, without AI (BDS held it with Slowness 255), on the same grass
//! floor at night. Each hit lands from the attacker position BDS recorded,
//! just before the tick whose move first carries it. Then every tick's
//! position and ground contact, and every position update and motion packet
//! the viewer gets about the mob, must equal BDS's. A replayed zombie gets
//! the knockback resistance BDS's zombie showed on its first hit (each
//! zombie draws its own); a replayed zoglin resists by its definition's
//! `minecraft:knockback_resistance` alone.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    Experience, GameMode, Health, HeldSlot, MainInventory, OnGround, Player, PlayerSession,
    Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::attributes::KnockbackResistance;
use crate::entity::mob::constants::{KNOCKBACK_HORIZONTAL, ZOMBIE_KNOCKBACK_RESISTANCE_BONUS};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

/// BDS stores positions as `f32`; a replay over a hundred ticks stays this
/// close to them (the worst of the eight trials is 9e-6).
const POSITION_TOLERANCE: f64 = 2.0e-5;

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-knockback.json")).unwrap()
    })
}

/// The zoglin, hoglin and iron golem trials (`--items resist`).
fn resistance_fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!(
            "testdata/bds-1.26.51-knockback-resistance.json"
        ))
        .unwrap()
    })
}

fn trial(key: &str) -> &'static Value {
    &fixture()["trials"][key]
}

/// The frozen trials: the knockback and the physics alone.
const FROZEN: [&str; 8] = [
    "cow|east|frozen",
    "cow|diagonal|frozen",
    "cow|away|frozen",
    "zombie|east|frozen",
    "zombie|east-sword|frozen",
    "cow|falling|frozen",
    "zombie|falling|frozen",
    "zombie|falling-2|frozen",
];

fn vec3(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

/// One position update or motion packet about the mob, tied to the tick
/// (sample index) it reports.
#[derive(Debug, Clone, PartialEq)]
enum Wire {
    Move {
        sample: usize,
        position: [Option<f64>; 3],
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
        match event.get("delta") {
            Some(delta) => Wire::Move {
                sample,
                position: std::array::from_fn(|axis| delta[axis].as_f64()),
                on_ground: event["ground"].as_bool().unwrap(),
                ticks: event["ticks"].as_u64().unwrap(),
            },
            None => Wire::Motion {
                sample,
                motion: vec3(&event["motion"]),
            },
        }
    }

    fn sample(&self) -> usize {
        match self {
            Wire::Move { sample, .. } | Wire::Motion { sample, .. } => *sample,
        }
    }

    fn matches(&self, other: &Wire) -> bool {
        match (self, other) {
            (
                Wire::Move {
                    sample: a,
                    position: pa,
                    on_ground: ga,
                    ticks: ta,
                },
                Wire::Move {
                    sample: b,
                    position: pb,
                    on_ground: gb,
                    ticks: tb,
                },
            ) => {
                a == b
                    && ga == gb
                    && ta == tb
                    && pa.iter().zip(pb).all(|(x, y)| match (x, y) {
                        (Some(x), Some(y)) => (x - y).abs() < POSITION_TOLERANCE,
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
            ) => a == b && (*ma - *mb).abs().max_element() < POSITION_TOLERANCE,
            _ => false,
        }
    }
}

/// What the replay of one trial produced.
struct Replay {
    /// Position and ground contact after each tick (index = BDS sample).
    positions: Vec<(DVec3, bool)>,
    wire: Vec<Wire>,
}

/// Replay a frozen trial: a grass floor at y = -61 (the BDS superflat) over
/// chunks -1..=1 × -1..=1 at midnight on easy, viewed by one creative
/// player; the mob spawns at the trial's spawn point without its brain.
fn replay(key: &str) -> Replay {
    replay_trial(key, trial(key))
}

fn replay_trial(key: &str, trial: &Value) -> Replay {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(7));
    *world.resource_mut::<Difficulty>() = Difficulty::Easy;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    crate::world::clock::set_time(world, 18000);
    let (tx, mut rx) = mpsc::channel::<McpePacket>(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                0.5,
                -60.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            GameMode::Creative,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "attacker".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, -61, z, *blocks::GRASS_BLOCK);
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let chunk = world.spawn((ChunkData::new(column), viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }

    let at = &trial["spec"]["at"];
    let spawn = vec3(at);
    let yaw = at[3].as_f64().unwrap() as f32;
    let mob = loop {
        let mob = spawn_mob(
            world,
            trial["mob"].as_str().unwrap(),
            spawn,
            yaw,
            SpawnCause::Spawned,
        )
        .unwrap();
        apply_pending_definitions(world);
        let baby = world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::BABY)
            .unwrap();
        if !baby {
            break mob;
        }
        world.despawn(mob);
    };
    world.entity_mut(mob).remove::<MobBrain>();
    let hits = trial["hits"].as_array().unwrap();
    if trial["mob"] == "minecraft:zombie" {
        // Every captured zombie was standing still or falling straight down
        // at its first hit, so that push is 0.4 × (1 − resistance).
        let push = vec3(&hits[0]["push"]);
        let resistance = 1.0 - push.x.hypot(push.z) / KNOCKBACK_HORIZONTAL;
        assert!((0.0..ZOMBIE_KNOCKBACK_RESISTANCE_BONUS).contains(&resistance));
        world.entity_mut(mob).insert(KnockbackResistance {
            spawn_bonus: Some(resistance as f32),
            ..KnockbackResistance::NONE
        });
    }
    let runtime_id = world.get::<RuntimeId>(mob).unwrap().0 as u64;

    let samples = trial["positions"].as_array().unwrap().len();
    let mut positions = vec![(spawn, false)];
    let mut wire = Vec::new();
    for sample in 1..samples {
        for hit in hits {
            if hit["sample"].as_u64().unwrap() as usize == sample {
                let source = vec3(&hit["source"]);
                let world = server.ecs.world_mut();
                world.get_mut::<Position>(player).unwrap().0 =
                    source + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
                super::player_attack(world, player, mob, &Default::default());
            }
        }
        server.tick();
        let world = server.ecs.world();
        positions.push((
            world.get::<Position>(mob).unwrap().0,
            world.get::<OnGround>(mob).unwrap().0,
        ));
        while let Ok(packet) = rx.try_recv() {
            match packet.data {
                McpePacketData::MoveActorDeltaPacket(moved)
                    if moved.move_data.actor_runtime_id.actor_runtime_id == runtime_id
                        && (moved.move_data.new_position_x.is_some()
                            || moved.move_data.new_position_y.is_some()
                            || moved.move_data.new_position_z.is_some()) =>
                {
                    let data = moved.move_data;
                    wire.push(Wire::Move {
                        sample,
                        position: [
                            data.new_position_x,
                            data.new_position_y,
                            data.new_position_z,
                        ]
                        .map(|axis| axis.map(f64::from)),
                        on_ground: data.is_on_ground,
                        ticks: data.ticks,
                    });
                }
                McpePacketData::SetActorMotionPacket(motion)
                    if motion.target_runtime_id.actor_runtime_id == runtime_id =>
                {
                    assert_eq!(
                        motion.tick.inputtick, 0,
                        "{key}: a mob's motion has no tick"
                    );
                    let motion = motion.motion;
                    wire.push(Wire::Motion {
                        sample,
                        motion: DVec3::new(motion.x.into(), motion.y.into(), motion.z.into()),
                    });
                }
                _ => {}
            }
        }
    }
    Replay { positions, wire }
}

/// The ticks a frozen trial's mob spent moving: from the first hit to the
/// last sample.
fn moving_samples(trial: &Value) -> std::ops::Range<usize> {
    let first = trial["hits"][0]["sample"].as_u64().unwrap() as usize;
    first..trial["positions"].as_array().unwrap().len()
}

#[test]
fn frozen_mobs_fly_and_land_like_bds_after_every_hit() {
    for key in FROZEN {
        let trial = trial(key);
        let replay = replay(key);
        for (sample, row) in trial["positions"].as_array().unwrap().iter().enumerate() {
            let bds = vec3(row);
            let bds_ground = row[3].as_u64().unwrap() == 1;
            let (position, on_ground) = replay.positions[sample];
            assert!(
                (position - bds).abs().max_element() < POSITION_TOLERANCE,
                "{key} tick {sample}: {position:?}, BDS {bds:?}"
            );
            // Ground contact once settled (BDS samples a freshly spawned
            // mob as grounded before its first move).
            if sample >= 2 {
                assert_eq!(on_ground, bds_ground, "{key} tick {sample}: on ground");
            }
        }
    }
}

#[test]
fn knockback_position_and_motion_packets_match_bds() {
    for key in FROZEN {
        let trial = trial(key);
        let replay = replay(key);
        let rows = trial["positions"].as_array().unwrap();
        let window = moving_samples(trial);
        // Position updates are compared on the ticks the mob moved more than
        // the update threshold (0.01 blocks) by itself. On slower ticks
        // whether one is sent depends on the last one sent, and the frozen
        // BDS mob still turned its head, which sends updates of its own.
        let moved = |sample: usize| {
            let step = |axis: usize| {
                rows[sample][axis].as_f64().unwrap() - rows[sample - 1][axis].as_f64().unwrap()
            };
            (0..3).map(|axis| step(axis).powi(2)).sum::<f64>() > 1.0e-4
        };
        let compared = |event: &Wire| match event {
            Wire::Move { sample, .. } => window.contains(sample) && moved(*sample),
            Wire::Motion { sample, .. } => window.contains(sample),
        };
        let bds: Vec<Wire> = trial["wire"]
            .as_array()
            .unwrap()
            .iter()
            .map(Wire::from_fixture)
            .filter(compared)
            .collect();
        let ours: Vec<&Wire> = replay.wire.iter().filter(|event| compared(event)).collect();
        assert!(bds.len() > 20, "{key}: {} packets compared", bds.len());
        assert_eq!(
            ours.len(),
            bds.len(),
            "{key}: packets\n ours {ours:#?}\n BDS {bds:#?}"
        );
        for (ours, bds) in ours.iter().zip(&bds) {
            assert!(ours.matches(bds), "{key}: {ours:?}, BDS {bds:?}");
        }
    }
}

/// Every summoned living actor with gravity starts with BDS's slight
/// downward velocity (its `AddActor` velocity and its first move); the
/// committed summon captures of the spawnable mobs carry it.
#[test]
fn spawned_mobs_start_with_the_native_downward_velocity() {
    use valentine::bedrock::codec::BedrockCodec as _;
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    let summons: [(&str, &[u8]); 5] = [
        (
            "minecraft:cow",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/cow.bin"
            ),
        ),
        (
            "minecraft:chicken",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/chicken.bin"
            ),
        ),
        (
            "minecraft:pig",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/pig.bin"
            ),
        ),
        (
            "minecraft:sheep",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/sheep.bin"
            ),
        ),
        (
            "minecraft:zombie",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/zombie.bin"
            ),
        ),
    ];
    for (identifier, bytes) in summons {
        assert_eq!(bytes[0], 0x0d, "{identifier}: an AddActor capture");
        let mut input = &bytes[1..];
        let native = jolyne::valentine::AddActorPacket::decode(&mut input, ()).unwrap();
        let mob = spawn_mob(
            world,
            identifier,
            DVec3::new(0.5, 80.0, 0.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let velocity = world
            .get::<crate::entity::components::Velocity>(mob)
            .unwrap()
            .0;
        assert_eq!(
            [velocity.x as f32, velocity.y as f32, velocity.z as f32],
            [native.velocity.x, native.velocity.y, native.velocity.z],
            "{identifier}"
        );
    }
}

#[test]
fn zombies_spawn_with_their_own_knockback_resistance_below_the_bonus() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(3));
    let spawn = |world: &mut World, identifier| {
        spawn_mob(
            world,
            identifier,
            DVec3::new(0.5, 80.0, 0.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap()
    };
    let mut seen = Vec::new();
    for _ in 0..64 {
        let zombie = spawn(world, "minecraft:zombie");
        let resistance = *world.get::<KnockbackResistance>(zombie).unwrap();
        assert_eq!(resistance.base, 0.0);
        let bonus = f64::from(resistance.spawn_bonus.unwrap());
        assert!((0.0..ZOMBIE_KNOCKBACK_RESISTANCE_BONUS).contains(&bonus));
        seen.push(bonus);
    }
    seen.sort_by(f64::total_cmp);
    assert!(
        seen[0] < 0.01 && seen[63] > 0.04,
        "spread over the range: {seen:?}"
    );
    let cow = spawn(world, "minecraft:cow");
    assert!(world.get::<KnockbackResistance>(cow).is_none());
    // The zoglin's definition gives it 0.6 and no spawn bonus.
    let zoglin = spawn(world, "minecraft:zoglin");
    assert_eq!(
        world.get::<KnockbackResistance>(zoglin),
        Some(&KnockbackResistance {
            base: 0.6,
            spawn_bonus: None
        })
    );
}

/// The frozen BDS zoglin, hit three times, flies and lands like the
/// replayed one, whose only resistance is its definition's 0.6: every
/// push and lift is 0.4 of a cow's. The iron golem's 1 stops BDS's
/// knockback altogether (no velocity change at all, not even the halving);
/// the golem cannot spawn here yet, so its rule is checked on the attribute.
#[test]
fn definition_knockback_resistance_scales_pushes_like_bds() {
    let key = "zoglin|east|frozen";
    let trial = &resistance_fixture()["trials"][key];
    let first = vec3(&trial["hits"][0]["push"]);
    assert!(
        (first.x - KNOCKBACK_HORIZONTAL * 0.4).abs() < 1e-6,
        "{first}"
    );
    let replay = replay_trial(key, trial);
    for (sample, row) in trial["positions"].as_array().unwrap().iter().enumerate() {
        let bds = vec3(row);
        let (position, on_ground) = replay.positions[sample];
        assert!(
            (position - bds).abs().max_element() < POSITION_TOLERANCE,
            "{key} tick {sample}: {position:?}, BDS {bds:?}"
        );
        if sample >= 2 {
            assert_eq!(
                on_ground,
                row[3].as_u64().unwrap() == 1,
                "{key} tick {sample}"
            );
        }
    }
    let golem = &resistance_fixture()["trials"]["iron_golem|east|frozen"];
    for hit in golem["hits"].as_array().unwrap() {
        assert_eq!(vec3(&hit["push"]), DVec3::ZERO, "the golem is not pushed");
    }
    let resistance = KnockbackResistance {
        base: 1.0,
        spawn_bonus: None,
    };
    assert_eq!(
        resistance.resist(crate::entity::mob::components::KnockbackPower::DEFAULT),
        None
    );
}

/// Prints how far each frozen replay is from BDS: worst position error, the
/// distance the first hit carried the mob, the motion packet of the first
/// hit, and how many compared packets differ. Run by hand:
/// `cargo test -p unastar --lib bds_knockback::report -- --ignored --nocapture`.
#[test]
#[ignore = "diagnostic report"]
fn report() {
    for key in FROZEN {
        let trial = trial(key);
        let replay = replay(key);
        let rows = trial["positions"].as_array().unwrap();
        let worst = rows
            .iter()
            .enumerate()
            .map(|(sample, row)| (replay.positions[sample].0 - vec3(row)).abs().max_element())
            .fold(0.0, f64::max);
        let hits: Vec<usize> = trial["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["sample"].as_u64().unwrap() as usize)
            .collect();
        let flight_end = hits.get(1).copied().unwrap_or(rows.len()) - 1;
        let carried = |at: DVec3| (at - vec3(&rows[hits[0] - 1])).with_y(0.0).length();
        let first_motion = |events: &[Wire]| {
            events.iter().find_map(|event| match event {
                Wire::Motion { sample, motion } if *sample == hits[0] => Some(*motion),
                _ => None,
            })
        };
        let bds: Vec<Wire> = trial["wire"]
            .as_array()
            .unwrap()
            .iter()
            .map(Wire::from_fixture)
            .collect();
        let differing = bds
            .iter()
            .filter(|event| event.sample() >= hits[0])
            .filter(|event| !replay.wire.iter().any(|ours| ours.matches(event)))
            .count();
        eprintln!(
            "{key}: worst error {worst:.2e}; first hit carried {:.3} (BDS {:.3}); its motion \
             {:?} (BDS {:?}); BDS packets without an equal one {differing}",
            carried(replay.positions[flight_end].0),
            carried(vec3(&rows[flight_end])),
            first_motion(&replay.wire),
            first_motion(&bds),
        );
    }
}
