//! The skeleton family's ranged attack against BDS 1.26.51
//! (`testdata/bds-1.26.51-ranged.json`: `tools/bds-sweep/sweep.py ranged
//! --client-capture`, `--items slice` on easy, normal and hard and the
//! `geometry`, `aim`, `spread`, `damage`, `damage-long`, `weapon` and
//! `filters` sets, distilled by `ranged_fixture.py`).
//!
//! Each shooting item is replayed with the same mob, frozen by a hidden
//! Slowness 255 as BDS's was, where BDS spawned it, at a survival player
//! standing where BDS's stood at the first shot. The spread, the hit damage
//! and the aim noise are random, so they are compared as distributions;
//! everything else is compared shot by shot.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::DataItemEntryPayload;
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    ActiveEffect, EffectType, Experience, GameMode, Health, HeldSlot, MainInventory, Player,
    PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use crate::world::rules::{Difficulty, GameRules};

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-ranged.json")).unwrap()
    })
}

fn vec3(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

fn difficulty(name: &str) -> Difficulty {
    match name {
        "easy" => Difficulty::Easy,
        "normal" => Difficulty::Normal,
        "hard" => Difficulty::Hard,
        other => panic!("difficulty {other}"),
    }
}

/// Every item of the fixture with its run name.
fn items() -> impl Iterator<Item = (&'static str, &'static str, &'static Value)> {
    fixture()["runs"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(run, items)| {
            items
                .as_object()
                .unwrap()
                .iter()
                .map(move |(key, item)| (run.as_str(), key.as_str(), item))
        })
}

struct Replay {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A stone floor at y = -61 over the chunks around the origin at midnight,
/// seen by one survival player whose feet are at `feet`, kept alive by
/// Resistance V and Regeneration V as BDS's sweep player was.
fn replay(level: Difficulty, feet: DVec3, seed: u64) -> Replay {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = level;
    world.resource_mut::<GameRules>().do_mob_spawning = false;
    crate::world::clock::set_time(world, 18_000);
    let (tx, rx) = mpsc::channel::<McpePacket>(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "sweep".into(), None, None, tx),
        ))
        .id();
    for cx in -2..=1 {
        for cz in -2..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, -61, z, *blocks::STONE);
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
    for effect in [EffectType::RESISTANCE, EffectType::REGENERATION] {
        crate::entity::effects::give(world, player, effect, ActiveEffect::new(4, 100_000));
    }
    let mut replay = Replay { server, player, rx };
    replay.packets();
    replay
}

impl Replay {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn packets(&mut self) -> Vec<McpePacket> {
        let mut out = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            out.push(packet);
        }
        out
    }

    /// The item's setup blocks (`{from, to, block}` boxes; a block with
    /// states is not replayed by these tests).
    fn blocks(&mut self, item: &Value) {
        for change in item["blocks"].as_array().unwrap() {
            assert!(change.get("states").is_none(), "block states: {change}");
            let (from, to) = (vec3(&change["from"]), vec3(&change["to"]));
            let name = change["block"].as_str().unwrap();
            let block = crate::registry::BlockRegistry::vanilla()
                .get_by_name(name)
                .unwrap_or_else(|| panic!("{name}"))
                .id;
            let (lo, hi) = (from.min(to).as_ivec3(), from.max(to).as_ivec3());
            let world = self.world();
            for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        set_block(world, x, y, z, block);
                    }
                }
            }
        }
    }

    /// The item's mob where BDS spawned it, frozen by a hidden Slowness 255.
    fn mob(&mut self, item: &Value) -> Entity {
        self.blocks(item);
        let at = &item["at"];
        let feet = vec3(at);
        let yaw = at[3].as_f64().unwrap() as f32;
        let identifier = item["mob"].as_str().unwrap();
        let world = self.world();
        let mob = spawn_mob(world, identifier, feet, yaw, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        let mut slow = ActiveEffect::new(255, 100_000);
        slow.show_particles = false;
        crate::entity::effects::give(world, mob, EffectType::SLOWNESS, slow);
        mob
    }
}

fn set_block(world: &mut World, x: i32, y: i32, z: i32, block: u32) {
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

/// An arrow's AddActor: position, velocity, rotation (pitch, yaw), owner,
/// `CUSTOM_DISPLAY`.
type Shot = (DVec3, DVec3, (f32, f32), i64, i8);

/// What one tick sent about the mob and its arrows.
#[derive(Debug, Default)]
struct Tick {
    target: Option<bool>,
    facing: Option<bool>,
    shaking: Option<bool>,
    /// AddActor of an arrow: position, velocity, rotation (pitch, yaw),
    /// owner, `CUSTOM_DISPLAY`.
    shots: Vec<Shot>,
    /// Level sounds: name, actor identifier, position.
    sounds: Vec<(String, String, DVec3)>,
    /// `MobEffect` the player got: event, effect, amplifier, duration.
    effects: Vec<(String, i32, i32, i32)>,
}

fn wire(v: &jolyne::valentine::types::Vec3) -> DVec3 {
    DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
}

fn watch(packets: Vec<McpePacket>, mob: i64) -> Tick {
    let mut tick = Tick::default();
    for packet in packets {
        match packet.data {
            McpePacketData::AddActorPacket(add) if add.actor_type == "minecraft:arrow" => {
                let value = |id: u32| {
                    add.actor_data
                        .data
                        .iter()
                        .find(|row| row.id == id)
                        .map(|row| row.payload.clone())
                };
                let owner = match value(ids::OWNER) {
                    Some(DataItemEntryPayload::DataItemInt64Payload(v)) => v.value,
                    other => panic!("arrow owner {other:?}"),
                };
                let display = match value(ids::CUSTOM_DISPLAY) {
                    Some(DataItemEntryPayload::DataItemBytePayload(v)) => v.value,
                    other => panic!("arrow custom display {other:?}"),
                };
                tick.shots.push((
                    wire(&add.position),
                    wire(&add.velocity),
                    (add.rotation.x, add.rotation.y),
                    owner,
                    display,
                ));
            }
            McpePacketData::LevelSoundEventPacket(sound) => tick.sounds.push((
                sound.sound_event.to_string(),
                sound.actor_identifier.to_string(),
                wire(&sound.position),
            )),
            McpePacketData::MobEffectPacket(effect)
                if effect.target_runtime_id.actor_runtime_id == 1 =>
            {
                tick.effects.push((
                    format!("{:?}", effect.event_id),
                    effect.effect_id,
                    effect.effect_amplifier,
                    effect.effect_duration_ticks,
                ))
            }
            McpePacketData::AddActorPacket(add)
                if add.target_runtime_id.actor_runtime_id == mob as u64 =>
            {
                read_data(&mut tick, &add.actor_data.data);
            }
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == mob as u64 =>
            {
                read_data(&mut tick, &data.actor_data.data);
            }
            _ => {}
        }
    }
    tick
}

/// The target, `SHAKING` and `FACING_TARGET_TO_RANGE_ATTACK` of a mob's
/// actor data rows.
fn read_data(tick: &mut Tick, rows: &[jolyne::valentine::types::DataItemEntry]) {
    for row in rows {
        let long = match &row.payload {
            DataItemEntryPayload::DataItemInt64Payload(v) => v.value,
            _ => continue,
        };
        match row.id {
            ids::TARGET => tick.target = Some(long != 0),
            ids::RESERVED_0 => tick.shaking = Some(long >> actor_flags::SHAKING & 1 == 1),
            ids::RESERVED_092 => {
                tick.facing =
                    Some(long >> (actor_flags::FACING_TARGET_TO_RANGE_ATTACK - 64) & 1 == 1)
            }
            _ => {}
        }
    }
}

/// The sample BDS's mob first took the player as its target.
fn bds_target(item: &Value) -> Option<u64> {
    item["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["target"] == true)
        .map(|e| e["t"].as_u64().unwrap())
}

/// Shooting items whose player was not moved before the shots compared:
/// the pinned items (`spread`, `aim`, `damage*`) and, of the others, the
/// shots before the first hit.
fn shooting_items() -> impl Iterator<Item = (&'static str, &'static str, &'static Value)> {
    items().filter(|(_, key, item)| {
        !item["mob"].is_null()
            && !item["shots"].as_array().unwrap().is_empty()
            && !key.contains("|hold-")
    })
}

/// Replay one item for `ticks`; the ticks' views.
fn run(item: &Value, ticks: usize, seed: u64) -> (Replay, Entity, i64, Vec<Tick>) {
    let first = &item["shots"][0];
    let feet = vec3(&first["player_feet"]);
    let level = difficulty(item["difficulty"].as_str().unwrap());
    let mut replay = replay(level, feet, seed);
    let mob = replay.mob(item);
    let id = replay.world().get::<RuntimeId>(mob).unwrap().0;
    replay.packets();
    let mut seen = vec![Tick::default()];
    for _ in 0..ticks {
        replay.server.tick();
        let packets = replay.packets();
        seen.push(watch(packets, id));
    }
    (replay, mob, id, seen)
}

/// The target is taken, the facing flag raised with it, and the shots
/// follow it at BDS's ticks: the first `interval + 2` ticks after the
/// target, then every `interval + 1` (61 / 71 ticks on easy and normal, 41
/// / 51 on hard for the skeleton and stray / the bogged and parched).
#[test]
fn shots_follow_the_target_at_the_bds_ticks() {
    let mut compared = 0;
    for (run_name, key, item) in shooting_items() {
        let shots: Vec<u64> = item["shots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["t"].as_u64().unwrap())
            .collect();
        let Some(target) = bds_target(item) else {
            continue;
        };
        // Up to three shots (the player stays in range in BDS for those).
        let wanted: Vec<u64> = shots.iter().take(3).map(|t| t - target).collect();
        let (_, _, _, seen) = run(item, (target + wanted.last().unwrap() + 4) as usize, 1);
        let ours_target =
            seen.iter()
                .position(|t| t.target == Some(true))
                .unwrap_or_else(|| panic!("{run_name} {key}: never targeted")) as u64;
        assert_eq!(
            seen[ours_target as usize].facing,
            Some(true),
            "{run_name} {key}: the facing flag rises with the target"
        );
        let ours: Vec<u64> = seen
            .iter()
            .enumerate()
            .flat_map(|(i, t)| std::iter::repeat_n(i as u64, t.shots.len()))
            .map(|t| t - ours_target)
            .take(wanted.len())
            .collect();
        assert_eq!(ours, wanted, "{run_name} {key}: shots after the target");
        compared += 1;
    }
    assert!(compared >= 20, "{compared} items compared");
}

/// The first shot of every shooting item: the arrow starts where BDS's did
/// (one block from the shooter toward the target, 1.61 up; to 1e-4), owned
/// by the shooter with its `aux_val`, its AddActor rotation is its heading,
/// and the batch sounds `disappeared` (the arrow, 0.9 above the shooter's
/// feet), the unnamed `undefined` and the shooter's `bow` (both at the
/// middle of the shooter's box) as BDS's did.
#[test]
fn the_first_shot_starts_and_sounds_like_bds() {
    let mut compared = 0;
    for (run_name, key, item) in shooting_items() {
        let bds = &item["shots"][0];
        let Some(target) = bds_target(item) else {
            continue;
        };
        let t = bds["t"].as_u64().unwrap() - target;
        let (_, _, id, seen) = run(item, (t + 12) as usize, 2);
        let (index, tick) = seen
            .iter()
            .enumerate()
            .find(|(_, t)| !t.shots.is_empty())
            .unwrap_or_else(|| panic!("{run_name} {key}: no shot"));
        let (position, velocity, (pitch, yaw), owner, display) = tick.shots[0];
        assert!(
            position.distance(vec3(&bds["position"])) < 1e-4,
            "{run_name} {key}: start {position} vs {}",
            bds["position"]
        );
        assert_eq!(owner, id, "{run_name} {key}: owner");
        assert_eq!(
            i64::from(display),
            bds["custom_display"].as_i64().unwrap(),
            "{run_name} {key}: aux"
        );
        let (heading_yaw, heading_pitch) = crate::entity::projectile::heading(velocity);
        let quantise = |degrees: f32| (degrees / (360.0 / 256.0)).trunc() * (360.0 / 256.0);
        assert_eq!(
            (pitch, yaw),
            (quantise(heading_pitch), quantise(heading_yaw)),
            "{run_name} {key}: rotation"
        );
        // Sounds: those of the shot batch and the next (unastar sends the
        // synchronised `disappeared` a batch later; see the quirks).
        let ours: Vec<&(String, String, DVec3)> = seen[index]
            .sounds
            .iter()
            .chain(seen[index + 1].sounds.iter())
            .filter(|(name, _, _)| ["disappeared", "undefined", "bow"].contains(&name.as_str()))
            .collect();
        let wanted = bds["sounds"].as_array().unwrap();
        assert_eq!(ours.len(), wanted.len(), "{run_name} {key}: {ours:?}");
        for want in wanted {
            let name = want["name"].as_str().unwrap();
            let (_, actor, position) = ours
                .iter()
                .find(|(n, _, _)| n == name)
                .unwrap_or_else(|| panic!("{run_name} {key}: no {name}"));
            assert_eq!(actor, want["actor"].as_str().unwrap(), "{key} {name}");
            assert!(
                position.distance(vec3(&want["position"])) < 1e-4,
                "{run_name} {key}: {name} at {position} vs {}",
                want["position"]
            );
        }
        compared += 1;
    }
    assert!(compared >= 20, "{compared} items compared");
}

/// Shots whose player stood where the sample says when BDS aimed: every
/// shot of a pinned item, the first of the others. With the item's
/// difficulty, the shooter's feet and each shot.
fn aimed_shots() -> Vec<(&'static str, Difficulty, DVec3, &'static Value)> {
    let mut out = Vec::new();
    for (_, key, item) in shooting_items() {
        let pinned = ["|spread", "|aim", "|damage"]
            .iter()
            .any(|case| key.contains(case));
        let level = difficulty(item["difficulty"].as_str().unwrap());
        let feet = vec3(&item["at"]);
        let shots = item["shots"].as_array().unwrap();
        let take = if pinned { shots.len() } else { 1 };
        for shot in shots.iter().take(take) {
            if !shot["player_feet"].is_null() {
                out.push((key, level, feet, shot));
            }
        }
    }
    out
}

/// The noise-free launch direction of a BDS shot: the parabola from the
/// shooter's position through the centre of the player's box (0.9 above
/// its feet), at the arrow's power 1.6 and gravity 0.05.
fn aimed(feet: DVec3, shot: &Value) -> DVec3 {
    let start = vec3(&shot["position"]);
    let centre = vec3(&shot["player_feet"]) + DVec3::Y * 0.9;
    super::ranged::aim_direction(feet, start, centre, 1.6, 0.05)
}

/// The aim: per item, BDS's shots climb (vy over the horizontal speed) as
/// the parabola through the centre of the target's box says, within three
/// standard errors of the item's mean (BDS adds a random spread to every
/// shot, so single shots differ); over all items the residuals are no
/// larger than the spread explains.
#[test]
fn shots_aim_along_the_bds_parabola() {
    use std::collections::BTreeMap;
    let mut per_item: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for (key, _, feet, shot) in aimed_shots() {
        let velocity = vec3(&shot["velocity"]);
        let climb = |v: DVec3| v.y / v.x.hypot(v.z);
        per_item
            .entry(key)
            .or_default()
            .push(climb(velocity) - climb(aimed(feet, shot)));
    }
    let mut chi = 0.0;
    let mut items = 0;
    for (key, residuals) in &per_item {
        if residuals.len() < 4 {
            continue;
        }
        let n = residuals.len() as f64;
        let mean = residuals.iter().sum::<f64>() / n;
        let sd = (residuals.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        let se = sd / n.sqrt();
        assert!(
            mean.abs() <= 3.0 * se + 1e-3,
            "{key}: mean climb residual {mean} (se {se}, {n} shots)"
        );
        chi += (mean / se).powi(2);
        items += 1;
    }
    assert!(items >= 10, "{items} items");
    assert!(
        chi / f64::from(items) < 2.0,
        "chi²/items {}",
        chi / f64::from(items)
    );
}

/// The spread: on each difficulty, every axis of BDS's shots strays from
/// the parabola by a gaussian of `RANGED_SPREAD_SCALE` × the uncertainty
/// (`16 - 4 × difficulty`): the root mean square deviation is that within
/// its sampling error.
#[test]
fn the_spread_grows_with_the_uncertainty_like_bds() {
    use crate::entity::mob::constants::RANGED_SPREAD_SCALE;
    for (level, uncertainty) in [
        (Difficulty::Easy, 12.0),
        (Difficulty::Normal, 8.0),
        (Difficulty::Hard, 4.0),
    ] {
        let mut sum = 0.0;
        let mut n = 0.0;
        for (_, shot_level, feet, shot) in aimed_shots() {
            if shot_level != level {
                continue;
            }
            let deviation = vec3(&shot["velocity"]) / 1.6 - aimed(feet, shot);
            sum += deviation.length_squared();
            n += 3.0;
        }
        let rms = (sum / n).sqrt();
        let expected = RANGED_SPREAD_SCALE * uncertainty;
        // The rms of n gaussian draws is off by about 1 / sqrt(2n).
        let tolerance = 3.0 / (2.0 * n).sqrt();
        assert!(
            (rms / expected - 1.0).abs() < tolerance,
            "{level:?}: rms {rms} vs {expected} over {n} axes"
        );
    }
}

/// The damage: every BDS arrow hit on the pinned player dealt
/// `ceil(speed × 2 × f)`, `f` uniform within `1 + 0.05 × difficulty ±
/// 0.225`: each hit's damage is one the model can deal at the arrow's speed,
/// and per difficulty the counts of 3, 4 and 5 match the model's
/// expectation (chi² over the outcomes).
#[test]
fn arrow_damage_varies_with_the_difficulty_like_bds() {
    use crate::entity::mob::constants::{SHOT_DAMAGE_PER_DIFFICULTY, SHOT_DAMAGE_SPREAD};
    for name in ["easy", "normal", "hard"] {
        let level = difficulty(name);
        let centre = 2.0 * (1.0 + SHOT_DAMAGE_PER_DIFFICULTY * f32::from(level as u8));
        let (lo, hi) = (
            centre - 2.0 * SHOT_DAMAGE_SPREAD,
            centre + 2.0 * SHOT_DAMAGE_SPREAD,
        );
        let mut observed = [0.0f64; 8];
        let mut expected = [0.0f64; 8];
        for (run, key, item) in items() {
            if !key.contains("|damage") || item["difficulty"] != name {
                continue;
            }
            let shots = item["shots"].as_array().unwrap();
            for hit in item["damage"].as_array().unwrap() {
                if hit["cause"] != "projectile" {
                    continue;
                }
                let t = hit["t"].as_i64().unwrap();
                let shot = shots
                    .iter()
                    .rev()
                    .find(|s| s["t"].as_i64().unwrap() <= t)
                    .unwrap();
                let mut v = vec3(&shot["velocity"]);
                for _ in 0..t - shot["t"].as_i64().unwrap() {
                    v *= 0.99;
                    v.y -= 0.05;
                }
                let speed = v.length() as f32;
                let damage = hit["amount"].as_f64().unwrap();
                // P(ceil(speed × f) = d) for f uniform in [lo, hi].
                let p = |d: f64| {
                    let (a, b) = ((d - 1.0) as f32 / speed, d as f32 / speed);
                    f64::from((b.min(hi) - a.max(lo)).max(0.0) / (hi - lo))
                };
                assert!(
                    p(damage) > 0.0,
                    "{run} {key}: {damage} at speed {speed} is out of the model"
                );
                observed[damage as usize] += 1.0;
                for (d, slot) in expected.iter_mut().enumerate() {
                    *slot += p(d as f64);
                }
            }
        }
        let chi: f64 = (0..8)
            .filter(|&d| expected[d] > 0.5)
            .map(|d| (observed[d] - expected[d]).powi(2) / expected[d])
            .sum();
        // Three outcomes (3, 4, 5): two degrees of freedom; 13.8 is p = 0.001.
        assert!(
            chi < 13.8,
            "{name}: chi² {chi}, observed {observed:?} expected {expected:?}"
        );
    }
}

/// A tipped arrow gives the player its effect as BDS's did: the stray's
/// long slowness (effect 2, 600 ticks), the bogged's poison (19, 112) and
/// the parched's weakness (18, 225), level 0, on the first hit (with up to
/// three seeds: on easy the spread can make every shot of a run miss).
#[test]
fn tipped_arrows_give_the_bds_effects() {
    let mut compared = 0;
    for (run_name, key, item) in items() {
        let Some(bds) = item["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["event"] == "Add" && e["effect"] != 10 && e["effect"] != 11)
        else {
            continue;
        };
        if !key.contains("|shoot") {
            continue;
        }
        // The spread decides whether a shot hits; a few seeds give a hit.
        let ours = (3..6)
            .find_map(|seed| {
                let (_, _, _, seen) = run(item, 300, seed);
                seen.iter()
                    .flat_map(|t| t.effects.iter())
                    .find(|(event, effect, _, _)| event == "Add" && *effect != 10 && *effect != 11)
                    .cloned()
            })
            .unwrap_or_else(|| panic!("{run_name} {key}: no effect"));
        assert_eq!(
            (ours.1, ours.2, ours.3),
            (
                bds["effect"].as_i64().unwrap() as i32,
                bds["amplifier"].as_i64().unwrap() as i32,
                bds["duration"].as_i64().unwrap() as i32
            ),
            "{run_name} {key}"
        );
        compared += 1;
    }
    assert!(compared >= 6, "{compared} items compared");
}

/// `has_ranged_weapon`: BDS's skeleton keeps shooting after its hand is
/// replaced with a bow, and never shoots once it holds a crossbow, a
/// trident, a snowball, a stick or nothing. Replayed with the hand replaced
/// on the same tick, the shots are the same: some with the bow, none
/// otherwise.
#[test]
fn only_a_bow_keeps_a_skeleton_shooting() {
    use crate::entity::mob::gear::{GearSlot, MobEquipment};
    let mut compared = 0;
    for (run_name, key, item) in items() {
        if !key.starts_with("skeleton|hold-") {
            continue;
        }
        let hand = &item["hand"][0];
        let swap = hand["t"].as_u64().unwrap();
        let level = difficulty(item["difficulty"].as_str().unwrap());
        let mut replay = replay(level, vec3(&item["player"]), 4);
        let mob = replay.mob(item);
        let id = replay.world().get::<RuntimeId>(mob).unwrap().0;
        replay.packets();
        let ticks = item["ticks"].as_u64().unwrap();
        let mut shots = 0;
        for t in 0..=ticks {
            if t == swap {
                let world = replay.world();
                let stack = match hand["item"].as_str() {
                    Some(name) => crate::server::game::world_stack(world, name, 1).unwrap(),
                    None => crate::item::ItemStack::empty(),
                };
                world
                    .get_mut::<MobEquipment>(mob)
                    .unwrap()
                    .set(GearSlot::Mainhand, stack, false);
            }
            replay.server.tick();
            shots += watch(replay.packets(), id).shots.len();
        }
        let bds = item["shots"].as_array().unwrap().len();
        assert_eq!(shots > 0, bds > 0, "{run_name} {key}: {shots} vs BDS {bds}");
        compared += 1;
    }
    assert_eq!(compared, 6);
}

/// `is_underground` as BDS answers it for a stray whose `change_to_skeleton`
/// turns it into a skeleton when underground, and one time in five anyway
/// (four repeats per roof): under stone 3 or 12 blocks up and under leaves
/// all four turned, under glass and the open sky at most one did.
#[test]
fn is_underground_is_the_bds_roof_rule() {
    use crate::entity::mob::events::MobFacts;
    use unastar_rules::filter::{Filter, FilterRegistry};
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let test = Filter::parse(
        &serde_json::json!({"test": "is_underground"}),
        FilterRegistry::vanilla(),
        "filters",
        &mut diagnostics,
    )
    .unwrap();
    // Strays given `change_to_skeleton`, grouped by their roof (the setup
    // blocks), with whether each one turned.
    let mut roofs = std::collections::BTreeMap::<String, (Vec<bool>, &Value)>::new();
    for (_, _, item) in items() {
        let fired = item["fired_events"].as_array().unwrap();
        if !fired.iter().any(|e| e["event"] == "change_to_skeleton") {
            continue;
        }
        let roof = roofs
            .entry(item["blocks"].to_string())
            .or_insert_with(|| (Vec::new(), item));
        roof.0.push(!item["gone"].is_null());
    }
    assert_eq!(roofs.len(), 5);
    for (roof, (turned, item)) in roofs {
        // Underground always turns; otherwise the event's random branch
        // turns one stray in five.
        let underground = turned.iter().all(|&t| t);
        let mut replay = replay(Difficulty::Normal, vec3(&item["player"]), 5);
        let mob = replay.mob(item);
        let world = replay.world();
        let facts = MobFacts {
            world,
            entity: mob,
            other: None,
        };
        assert_eq!(test.evaluate(&facts), underground, "{roof}");
    }
}

/// `in_block`: BDS's skeleton with powder snow at its feet, or at its head
/// only, shakes (`is_shaking` from its `got_in_powder_snow` group) one
/// tick after it spawned; so does ours, on the same tick.
#[test]
fn powder_snow_anywhere_in_the_box_shakes_a_skeleton_like_bds() {
    let mut compared = 0;
    for (run_name, key, item) in items() {
        if !key.starts_with("skeleton|powder-") {
            continue;
        }
        let bds = item["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["shaking"] == true)
            .unwrap_or_else(|| panic!("{key}: BDS never shook"))["t"]
            .as_u64()
            .unwrap();
        let mut replay = replay(Difficulty::Normal, vec3(&item["player"]), 6);
        let mob = replay.mob(item);
        let id = replay.world().get::<RuntimeId>(mob).unwrap().0;
        replay.packets();
        // Tick `t` of BDS's samples is our tick `t + 1` (tick 0 sends the
        // AddActor).
        let mut ours = None;
        for t in 0..10 {
            replay.server.tick();
            if watch(replay.packets(), id).shaking == Some(true) && ours.is_none() {
                ours = Some(t);
            }
        }
        assert_eq!(ours, Some(bds), "{run_name} {key}");
        compared += 1;
    }
    assert_eq!(compared, 2);
}

/// The skeleton's `minecraft:lightning_immune` damage sensor filters on
/// `other_with_families`, which is no filter key: in BDS a skeleton given
/// `minecraft:spring_trap` still took its `entity_attack` and `fall` hits
/// in full, as the control skeleton did. Ours compiles that filter to one
/// that never matches, so the hits land the same.
#[test]
fn the_spring_trap_sensor_blocks_no_hit_like_bds() {
    use crate::entity::mob::combat::{Hit, hurt_mob};
    use crate::entity::mob::{MobKind, events::fire_event};
    use unastar_data::actor_components::body::ActorDamageCause;
    for (_, key, item) in items() {
        if !key.starts_with("skeleton|spring-trap-") {
            continue;
        }
        let trap = item["fired_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "minecraft:spring_trap");
        let mut replay = replay(Difficulty::Normal, vec3(&item["player"]), 7);
        let mob = replay.mob(item);
        if trap {
            let world = replay.world();
            let handle = world
                .get::<MobKind>(mob)
                .unwrap()
                .0
                .events
                .event("minecraft:spring_trap")
                .unwrap();
            fire_event(world, mob, &handle, None).unwrap();
            apply_pending_definitions(world);
        }
        let mut dealt = Vec::new();
        for hit in item["damage"].as_array().unwrap() {
            if hit["who"] != "mob" {
                continue;
            }
            let (cause, amount) = match hit["cause"].as_str().unwrap() {
                "entity_attack" => (ActorDamageCause::EntityAttack, 4.0),
                "fall" => (ActorDamageCause::Fall, 3.0),
                other => panic!("{key}: cause {other}"),
            };
            let world = replay.world();
            let before = world.get::<Health>(mob).unwrap().current;
            hurt_mob(
                world,
                mob,
                Hit {
                    damage: amount,
                    cause,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
                },
            );
            let after = replay.world().get::<Health>(mob).unwrap().current;
            dealt.push((before - after, hit["amount"].as_f64().unwrap() as f32));
            // Past the hurt cooldown before the next hit.
            for _ in 0..20 {
                replay.server.tick();
            }
        }
        assert_eq!(dealt.len(), 2, "{key}");
        for (ours, bds) in dealt {
            assert_eq!(ours, bds, "{key}");
        }
    }
}

/// `behavior.avoid_mob_type`: BDS's skeleton two blocks from a frozen wolf
/// (no target: the player is creative and far) starts walking away on its
/// third tick, about 3.75 blocks every 20 ticks, and only stops where its
/// random flee path ends, 14 to 18 blocks from the wolf: past `max_flee`
/// (10; twenty more BDS flights, `avoid-many`, all ended 12 to 21 blocks
/// from it). Wolves are not spawnable here, so a frozen cow in the `wolf`
/// family stands in. Ours flees the same way: moving by BDS's third tick
/// plus one, as fast to within 20 %, and to beyond `max_flee`.
#[test]
fn a_skeleton_flees_a_wolf_like_bds() {
    use crate::entity::definition::families::ActorFamilies;
    let mut runs = 0;
    for (_, key, item) in items() {
        if !key.starts_with("skeleton|avoid-") || item["ticks"] != 200 {
            continue;
        }
        let bds: Vec<DVec3> = item["positions"]
            .as_array()
            .unwrap()
            .iter()
            .map(vec3)
            .collect();
        let wolf = DVec3::new(2.5, -60.0, 6.5);
        let start = bds[0];
        let bds_first = bds.iter().position(|p| p.distance(start) > 1e-3).unwrap();
        let bds_speed = (bds[60] - bds[20]).length() / 40.0;
        let bds_end = bds.last().unwrap().distance(wolf);

        let mut replay = replay(Difficulty::Normal, vec3(&item["player"]), 8 + runs);
        let player = replay.player;
        *replay.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
        let world = replay.world();
        let cow = spawn_mob(world, "minecraft:cow", wolf, 0.0, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        let registry = world
            .resource::<crate::entity::mob::MobRegistry>()
            .families()
            .clone();
        world
            .entity_mut(cow)
            .insert(ActorFamilies::of(&registry, &["wolf".to_owned()]));
        world
            .entity_mut(cow)
            .remove::<crate::entity::mob::ai::MobBrain>();
        let item_unfrozen = item;
        let mob = {
            let at = &item_unfrozen["at"];
            let world = replay.world();
            let mob = spawn_mob(
                world,
                "minecraft:skeleton",
                vec3(at),
                at[3].as_f64().unwrap() as f32,
                SpawnCause::Spawned,
            )
            .unwrap();
            apply_pending_definitions(world);
            mob
        };
        let mut ours = Vec::new();
        for _ in 0..bds.len() {
            ours.push(replay.world().get::<Position>(mob).unwrap().0);
            replay.server.tick();
        }
        if std::env::var("AVOID_DEBUG").is_ok() {
            println!(
                "{key} ours {:?}",
                ours.iter()
                    .step_by(10)
                    .map(|p| (
                        (p.distance(wolf) * 100.0).round() / 100.0,
                        (p.x * 10.0).round() / 10.0,
                        (p.z * 10.0).round() / 10.0
                    ))
                    .collect::<Vec<_>>()
            );
        }
        let first = ours.iter().position(|p| p.distance(start) > 1e-3);
        assert!(
            first.is_some_and(|t| t <= bds_first + 1),
            "{key}: ours first moved at {first:?}, BDS at {bds_first}"
        );
        let speed = (ours[60] - ours[20]).length() / 40.0;
        assert!(
            (speed / bds_speed - 1.0).abs() < 0.2,
            "{key}: {speed} blocks/tick vs BDS {bds_speed}"
        );
        let end = ours.last().unwrap().distance(wolf);
        assert!(
            end >= 10.0 && bds_end >= 10.0,
            "{key}: ended {end} from the wolf (BDS {bds_end})"
        );
        runs += 1;
    }
    assert_eq!(runs, 3);
}

/// Where a flight ends: every one of BDS's twenty `avoid-many` skeletons
/// stopped at least `max_flee` (10) from the wolf, and so does ours over
/// twenty seeds.
#[test]
fn flights_end_beyond_max_flee_like_bds() {
    use crate::entity::definition::families::ActorFamilies;
    let wolf = DVec3::new(2.5, -60.0, 6.5);
    let mut bds = 0;
    for (_, key, item) in items() {
        if key.starts_with("skeleton|avoid-") && item["ticks"] == 120 {
            let end = vec3(item["positions"].as_array().unwrap().last().unwrap());
            assert!(end.distance(wolf) >= 10.0, "BDS {key}");
            bds += 1;
        }
    }
    assert_eq!(bds, 20);
    let item = items()
        .find(|(_, key, item)| key.starts_with("skeleton|avoid-") && item["ticks"] == 120)
        .unwrap()
        .2;
    for seed in 0..20 {
        let mut replay = replay(Difficulty::Normal, vec3(&item["player"]), 100 + seed);
        let player = replay.player;
        *replay.world().get_mut::<GameMode>(player).unwrap() = GameMode::Creative;
        let world = replay.world();
        let cow = spawn_mob(world, "minecraft:cow", wolf, 0.0, SpawnCause::Spawned).unwrap();
        apply_pending_definitions(world);
        let registry = world
            .resource::<crate::entity::mob::MobRegistry>()
            .families()
            .clone();
        world
            .entity_mut(cow)
            .insert(ActorFamilies::of(&registry, &["wolf".to_owned()]));
        world
            .entity_mut(cow)
            .remove::<crate::entity::mob::ai::MobBrain>();
        let mob = spawn_mob(
            world,
            "minecraft:skeleton",
            vec3(&item["at"]),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        apply_pending_definitions(world);
        for _ in 0..120 {
            replay.server.tick();
        }
        let end = replay.world().get::<Position>(mob).unwrap().0;
        assert!(end.distance(wolf) >= 10.0, "seed {seed}: {end}");
    }
}

/// Arrow pickup: in BDS a dispensed arrow that stuck next to the survival
/// player was taken exactly when its shake (`Shake` 7, the arrow's 0.35 s
/// `stick_in_ground`) ended, and one stuck while the player stood away was
/// taken on the tick the player arrived. Ours drops an arrow carrying an
/// arrow item into the floor beside the player and counts the same ticks.
#[test]
fn a_stuck_arrow_is_picked_up_once_its_shake_ends_like_bds() {
    use crate::entity::projectile::ProjectileItem;
    let mut near = 0;
    let mut late = 0;
    for (_, key, item) in items() {
        if !item["mob"].is_null() || !key.starts_with("arrow|pickup-") {
            continue;
        }
        let shot = &item["shots"][0];
        let (Some(stuck), Some(taken)) = (shot["stuck"].as_u64(), shot["taken"].as_u64()) else {
            continue; // The arrow hit the player instead.
        };
        let shake = shot["shake"].as_u64().unwrap();
        let arrive = item["teleports"]
            .as_array()
            .unwrap()
            .first()
            .map(|move_to| move_to["t"].as_u64().unwrap());
        let beside = DVec3::new(0.5, -60.0, 0.5);
        let start = vec3(&item["player"]);
        let mut replay = replay(Difficulty::Normal, start, 9);
        let world = replay.world();
        let arrow = spawn_mob(
            world,
            "minecraft:arrow",
            DVec3::new(1.5, -57.5, 0.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let stack = crate::server::game::world_stack(world, "minecraft:arrow", 1).unwrap();
        world.entity_mut(arrow).insert((
            ProjectileItem(stack),
            crate::entity::components::Velocity(DVec3::new(0.0, -0.5, 0.0)),
        ));
        replay.packets();
        let (mut ours_stuck, mut ours_taken) = (None, None);
        for t in 0..200u64 {
            if let Some(arrive) = arrive
                && ours_stuck.is_some_and(|s| t == s + (arrive - stuck))
            {
                let player = replay.player;
                replay.world().get_mut::<Position>(player).unwrap().0 =
                    beside + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
            }
            replay.server.tick();
            let world = replay.world();
            if ours_stuck.is_none()
                && world
                    .get::<crate::entity::projectile::Stuck>(arrow)
                    .is_some()
            {
                ours_stuck = Some(t);
            }
            for packet in replay.packets() {
                if matches!(packet.data, McpePacketData::TakeItemActorPacket(_)) {
                    ours_taken.get_or_insert(t);
                }
            }
            if ours_taken.is_some() {
                break;
            }
        }
        let ours_stuck = ours_stuck.unwrap_or_else(|| panic!("{key}: never stuck"));
        let ours_taken = ours_taken.unwrap_or_else(|| panic!("{key}: never taken"));
        assert_eq!(
            ours_taken - ours_stuck,
            taken - stuck,
            "{key}: taken after the stick"
        );
        if arrive.is_none() {
            assert_eq!(taken - stuck, shake, "BDS {key}: the shake");
            near += 1;
        } else {
            assert_eq!(Some(taken), arrive, "BDS {key}: taken on arrival");
            late += 1;
        }
    }
    assert_eq!((near, late), (2, 3));
}
