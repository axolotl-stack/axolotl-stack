//! The drowned replayed against BDS 1.26.51
//! (`mob/ai/testdata/bds-1.26.51-drowned.json`, distilled by
//! `tools/bds-sweep/drowned_fixture.py` from `sweep.py drowned`): its
//! summon, sinking and walking the floor of a pool, swimming at a target
//! above it, its trident throws, its daytime targeting, its spawn
//! equipment (by distribution) and the zombie and husk conversions.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use serde::Deserialize;

use super::facets::{Arena, arena, block};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, Health, OnGround, Position};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::gear::GearSlot;
use crate::entity::mob::*;
use crate::world::rules::Difficulty;

const DROWNED: &str = "minecraft:drowned";
/// BDS's superflat floor is at y = -61, the arena's at 63.
pub(super) const DY: f64 = 124.0;
/// Where the hunting sweeps summoned the drowned, 12 blocks across from
/// the player (the `within_radius` of its target goal).
const HUNT_FROM: DVec3 = DVec3::new(16.5, 64.0, 0.5);

#[derive(Debug, Deserialize)]
struct Fixture {
    items: Vec<Item>,
    equipment: Equipment,
}

#[derive(Debug, Deserialize)]
struct Item {
    key: String,
    kind: String,
    player: [f64; 3],
    #[serde(default)]
    trace: Vec<(u32, f64, f64, f64, f32, f32, u8)>,
    #[serde(default)]
    throws: Vec<Throw>,
    #[serde(default)]
    damage: Vec<Damage>,
    #[serde(default)]
    converted: Option<Converted>,
}

#[derive(Debug, Deserialize)]
struct Throw {
    t: u32,
}

#[derive(Debug, Deserialize)]
struct Damage {
    amount: f32,
}

#[derive(Debug, Deserialize)]
struct Converted {
    last: u32,
    first: u32,
    into: String,
    #[serde(default)]
    held: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Equipment {
    total: u32,
    mainhand: std::collections::BTreeMap<String, u32>,
    offhand: std::collections::BTreeMap<String, u32>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-drowned.json"))
        .expect("drowned fixture decodes")
}

fn items<'a>(fixture: &'a Fixture, kind: &str) -> Vec<&'a Item> {
    fixture
        .items
        .iter()
        .filter(|item| item.kind == kind)
        .collect()
}

/// The sweep's pool (`sweep.py`'s `POOL`): glass x 2..30, y -61..-54,
/// z -8..8, water x 3..19, y -60..-54, z -7..7, raised by [`DY`]; with
/// `pillar` the glass pillar under the player at (4, -60..-55, 0).
pub(super) fn pool(seed: u64, mode: GameMode, pillar: bool) -> Arena {
    let glass = block("minecraft:glass");
    let water = block("minecraft:water");
    let air = block("minecraft:air");
    let dy = DY as i32;
    arena(seed, Difficulty::Normal, mode, move |pos: IVec3| {
        let (x, y, z) = (pos.x, pos.y - dy, pos.z);
        if pillar && x == 4 && z == 0 && (-60..=-55).contains(&y) {
            return Some(glass);
        }
        if (3..=19).contains(&x) && (-60..=-54).contains(&y) && (-7..=7).contains(&z) {
            return Some(water);
        }
        if (2..=30).contains(&x) && (-61..=-54).contains(&y) && (-8..=8).contains(&z) {
            return Some(glass);
        }
        (y > -54 && y <= -50 && (2..=30).contains(&x) && (-8..=8).contains(&z)).then_some(air)
    })
}

fn raised(bds: [f64; 3]) -> DVec3 {
    DVec3::new(bds[0], bds[1] + DY, bds[2])
}

/// A mob of `identifier` spawned with `event` (`summon … <event>`).
pub(super) fn summon(
    a: &mut Arena,
    identifier: &str,
    event: &str,
    feet: DVec3,
    yaw: f32,
) -> Entity {
    let reference: ActorReference = format!("{identifier}<{event}>").parse().unwrap();
    let world = a.world();
    let mob = spawn_mob_as(world, &reference, feet, yaw, SpawnCause::Spawned)
        .unwrap_or_else(|error| panic!("{identifier}: {error}"));
    apply_pending_definitions(world);
    mob
}

/// Every tick: the mob's feet and ground contact.
fn run(a: &mut Arena, mob: Entity, ticks: u32) -> Vec<(DVec3, bool)> {
    (0..ticks)
        .map(|_| {
            a.ticks(1);
            let world = a.world();
            (
                world.get::<Position>(mob).unwrap().0,
                world.get::<OnGround>(mob).is_some_and(|g| g.0),
            )
        })
        .collect()
}

/// The BDS trace as feet raised into the arena.
fn bds_feet(item: &Item) -> Vec<DVec3> {
    item.trace
        .iter()
        .map(|row| DVec3::new(row.1, row.2 + DY, row.3))
        .collect()
}

#[test]
fn spawned_drowned_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/drowned.bin"
    ));
    let mut a = pool(1, GameMode::Creative, false);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob = a.adult(DROWNED, feet);
    assert_add_actor_matches(a.world(), DROWNED, mob, &native);
}

/// Sinking in still water with no target: `v = 0.8 v − 0.02`, so 0.1 a
/// tick at most, tick for tick with BDS until it reaches the floor.
#[test]
fn an_idle_drowned_sinks_like_bds() {
    let fixture = fixture();
    for item in items(&fixture, "sink") {
        let bds = bds_feet(item);
        let mut a = pool(2, GameMode::Creative, false);
        a.move_player(raised(item.player));
        // The sweep summoned at y -56 and sampled from about ten ticks
        // later: line the runs up on BDS's first fall.
        let mob = summon(
            &mut a,
            DROWNED,
            "minecraft:as_adult",
            DVec3::new(11.5, -56.0 + DY, 0.5),
            90.0,
        );
        let ours: Vec<DVec3> = run(&mut a, mob, 14 + 40)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        let (ours, bds) = (moves(&ours), moves(&bds));
        let offset = (0..14)
            .min_by(|&i, &j| {
                (ours[i].y - bds[0].y)
                    .abs()
                    .total_cmp(&(ours[j].y - bds[0].y).abs())
            })
            .unwrap();
        // Until BDS's drowned starts strolling or lands.
        let falling = bds
            .iter()
            .take_while(|m| m.x == 0.0 && m.z == 0.0 && m.y < -0.08)
            .count();
        assert!(falling >= 5, "{}: BDS fell {falling} ticks", item.key);
        for t in 0..falling {
            assert!(
                (ours[offset + t].y - bds[t].y).abs() < 5e-4,
                "{} t {t}: fell {} vs BDS {}",
                item.key,
                ours[offset + t].y,
                bds[t].y
            );
        }
    }
}

/// Per-tick moves of a trace.
fn moves(feet: &[DVec3]) -> Vec<DVec3> {
    feet.windows(2).map(|w| w[1] - w[0]).collect()
}

/// A hunting drowned on the pool floor walks at the water speed: each tick
/// the speed attribute × 0.98 × 0.06 (its native underwater movement) is
/// added before the 0.8 water drag, 0.0676 a tick at full speed.
#[test]
fn a_hunting_drowned_walks_the_pool_floor_at_bds_speed() {
    let fixture = fixture();
    let item = items(&fixture, "hunt_floor")[0];
    let bds = moves(&bds_feet(item));
    let bds_top = bds[30..100].iter().map(|m| m.x.abs()).fold(0.0, f64::max);
    let mut a = pool(3, GameMode::Survival, false);
    a.move_player(raised(item.player));
    let mob = summon(&mut a, DROWNED, "minecraft:as_adult", HUNT_FROM, 90.0);
    let ours: Vec<DVec3> = run(&mut a, mob, 120).into_iter().map(|(f, _)| f).collect();
    let ours = moves(&ours);
    let top = ours[60..110].iter().map(|m| m.x.abs()).fold(0.0, f64::max);
    assert!(
        (top - bds_top).abs() < 1e-3,
        "full speed {top} vs BDS {bds_top}"
    );
    // The push ramps by a tenth of the full push a tick, as BDS's does.
    let start = ours
        .iter()
        .position(|m| m.x.abs() > 1e-6)
        .expect("it walks");
    let gains: Vec<f64> = (start + 1..start + 6)
        .map(|t| ours[t].x - 0.8 * ours[t - 1].x)
        .collect();
    let bds_gains: Vec<f64> = (1..6).map(|t| bds[t].x - 0.8 * bds[t - 1].x).collect();
    let steps = |g: &[f64]| g.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f64>>();
    for (ours, theirs) in steps(&gains).iter().zip(steps(&bds_gains)) {
        assert!(
            (ours - theirs).abs() < 2e-4,
            "{gains:?} vs BDS {bds_gains:?}"
        );
    }
}

/// A drowned hunting a player who stands on a pillar at the surface swims
/// straight at the player's feet with no gravity, pushed 0.0135 a tick
/// along the line (BDS: (−0.0605, 0.0302) a tick), and ends on the pillar.
#[test]
fn a_hunting_drowned_swims_straight_at_a_target_above_it() {
    let fixture = fixture();
    for item in items(&fixture, "hunt_surface") {
        let bds = moves(&bds_feet(item));
        let mut a = pool(4, GameMode::Survival, true);
        a.move_player(raised(item.player));
        let mob = summon(&mut a, DROWNED, "minecraft:as_adult", HUNT_FROM, 90.0);
        let ours: Vec<(DVec3, bool)> = run(&mut a, mob, 260);
        let feet: Vec<DVec3> = ours.iter().map(|(f, _)| *f).collect();
        let steps = moves(&feet);
        // The steady swim: BDS's move once it reached full speed.
        let bds_swim = bds[60];
        let swim = steps
            .iter()
            .find(|m| (m.length() - bds_swim.length()).abs() < 2e-3 && m.y > 0.0)
            .unwrap_or_else(|| panic!("{}: no steady swim in {steps:?}", item.key));
        assert!(
            (swim.x - bds_swim.x).abs() < 3e-3 && (swim.y - bds_swim.y).abs() < 3e-3,
            "{}: swim {swim} vs BDS {bds_swim}",
            item.key
        );
        // BDS climbs onto the pillar top (y -54) within the trace; unastar's
        // drowned reaches the player at the surface beside the pillar (it
        // counts the last waypoint reached within a block and stops there:
        // an open item).
        let top = -54.0 + DY;
        let on_pillar = |f: &DVec3| (f.y - top).abs() < 1e-3 && f.x < 5.3;
        assert!(
            bds_feet(item).iter().any(on_pillar),
            "{}: BDS never climbs",
            item.key
        );
        let player = raised(item.player);
        assert!(
            feet.iter()
                .any(|f| f.y >= top && (f.x - player.x).abs() < 1.0),
            "{}: never reached the player: {:?}",
            item.key,
            feet.last()
        );
    }
}

/// The ticks thrown tridents appeared on, counted from the summon.
fn trident_throws(a: &mut Arena, ticks: u32) -> (Vec<u32>, Vec<f32>) {
    let mut seen = std::collections::BTreeSet::new();
    let mut throws = Vec::new();
    let player = a.player;
    let mut last_health = a.world().get::<Health>(player).unwrap().current;
    let mut hurts = Vec::new();
    for t in 0..ticks {
        a.ticks(1);
        let world = a.world();
        let mut query = world.query::<(Entity, &MobKind)>();
        for (entity, kind) in query.iter(world) {
            if &*kind.0.identifier == "minecraft:thrown_trident" && seen.insert(entity) {
                throws.push(t);
            }
        }
        let health = world.get::<Health>(player).unwrap().current;
        if health < last_health {
            hurts.push(last_health - health);
        }
        // Keep the player alive and at full health, as the sweep's
        // Health Boost and Regeneration did.
        world.get_mut::<Health>(player).unwrap().current = 20.0;
        last_health = 20.0;
    }
    (throws, hurts)
}

fn intervals(ticks: &[u32]) -> Vec<u32> {
    ticks.windows(2).map(|w| w[1] - w[0]).collect()
}

/// A ranged drowned 10 blocks from a player in the water throws a trident
/// every ~59 ticks (the reload scaled to the range's end), each dealing 8.
/// Compared by distribution over seeded runs.
#[test]
fn a_ranged_drowned_throws_tridents_like_bds() {
    let fixture = fixture();
    let bds: Vec<u32> = items(&fixture, "trident_water")
        .iter()
        .flat_map(|item| intervals(&item.throws.iter().map(|t| t.t).collect::<Vec<_>>()))
        .collect();
    let bds_damage: Vec<f32> = items(&fixture, "trident_water")
        .iter()
        .flat_map(|item| item.damage.iter().map(|d| d.amount))
        .collect();
    assert!(bds_damage.iter().all(|d| *d == 8.0), "{bds_damage:?}");
    let mut ours = Vec::new();
    let mut hurt = Vec::new();
    for seed in 0..4 {
        let mut a = pool(10 + seed, GameMode::Survival, false);
        a.move_player(DVec3::new(4.5, 64.0, 0.5));
        summon(
            &mut a,
            DROWNED,
            "minecraft:as_ranged_adult",
            DVec3::new(14.5, 64.0, 0.5),
            90.0,
        );
        let (throws, hurts) = trident_throws(&mut a, 610);
        ours.extend(intervals(&throws));
        hurt.extend(hurts);
    }
    let mean = |v: &[u32]| v.iter().sum::<u32>() as f64 / v.len().max(1) as f64;
    assert!(ours.len() >= bds.len() / 2, "ours {ours:?} vs BDS {bds:?}");
    assert!(
        (mean(&ours) - mean(&bds)).abs() < 3.0,
        "intervals {ours:?} vs BDS {bds:?}"
    );
    assert!(hurt.iter().all(|h| (*h - 8.0).abs() < 1e-3), "{hurt:?}");
}

/// By day a drowned hunts a player in the water and ignores one on land.
#[test]
fn by_day_a_drowned_hunts_only_players_in_the_water() {
    let fixture = fixture();
    for (kind, hunted) in [("day_water", true), ("day_land", false)] {
        for item in items(&fixture, kind) {
            assert_eq!(!item.damage.is_empty(), hunted, "BDS {}", item.key);
            let mut a = pool(20, GameMode::Survival, false);
            crate::world::clock::set_time(a.world(), 6000);
            a.move_player(raised(item.player));
            let mob = summon(&mut a, DROWNED, "minecraft:as_adult", HUNT_FROM, 90.0);
            a.ticks(200);
            let target = a
                .world()
                .get::<super::super::state::AttackTarget>(mob)
                .is_some();
            assert_eq!(target, hunted, "{}", item.key);
        }
    }
}

/// The equipment a spawning drowned draws: a trident in 1 of 16 (the ranged
/// branches), a fishing rod in 3.75 % of the rest, a nautilus shell in the
/// off hand in 8 %; compared with the 200 BDS summons by binomial bounds.
#[test]
fn drowned_spawn_equipment_matches_bds_by_distribution() {
    let fixture = fixture();
    let bds = &fixture.equipment;
    let mut a = pool(30, GameMode::Creative, false);
    let n = 2000;
    let mut mainhand = std::collections::BTreeMap::<String, u32>::new();
    let mut offhand = std::collections::BTreeMap::<String, u32>::new();
    for _ in 0..n {
        let world = a.world();
        let mob = spawn_mob(
            world,
            DROWNED,
            DVec3::new(-6.5, 64.0, 0.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        apply_pending_definitions(world);
        let equipment = world.get::<gear::MobEquipment>(mob).unwrap();
        for (slot, counts) in [
            (GearSlot::Mainhand, &mut mainhand),
            (GearSlot::Offhand, &mut offhand),
        ] {
            let stack = equipment.get(slot);
            let name = if stack.is_empty() {
                "minecraft:air"
            } else {
                stack.item_id()
            };
            *counts.entry(name.to_owned()).or_default() += 1;
        }
        world.despawn(mob);
    }
    for (ours, theirs) in [(&mainhand, &bds.mainhand), (&offhand, &bds.offhand)] {
        for (name, count) in theirs {
            let p = f64::from(*count) / f64::from(bds.total);
            let q = f64::from(ours.get(name).copied().unwrap_or(0)) / f64::from(n);
            // Four standard errors of the BDS share.
            let bound = 4.0 * (p.max(0.02) * (1.0 - p) / f64::from(bds.total)).sqrt();
            assert!((p - q).abs() < bound, "{name}: ours {q} vs BDS {p}");
        }
        assert!(
            ours.keys().all(|name| theirs.contains_key(name)),
            "{ours:?} vs {theirs:?}"
        );
    }
}

/// A zombie held under water turns into a drowned (a husk into a zombie)
/// 902 ticks after its summon (BDS: 30 s of the underwater timer, then the
/// 15 s transformation delay), at the same tick in every run, and the
/// drowned keeps the zombie's sword.
#[test]
fn zombies_and_husks_under_water_convert_when_bds_converts_them() {
    let fixture = fixture();
    for kind in ["convert_zombie", "convert_baby", "convert_husk"] {
        for item in items(&fixture, kind) {
            let converted = item.converted.as_ref().expect("BDS converted it");
            assert_eq!(converted.last + 1, converted.first, "{}", item.key);
            let (from, event) = match kind {
                "convert_husk" => ("minecraft:husk", "minecraft:as_adult"),
                "convert_baby" => ("minecraft:zombie", "minecraft:as_baby"),
                _ => ("minecraft:zombie", "minecraft:as_adult"),
            };
            let mut a = pool(40, GameMode::Creative, false);
            a.move_player(raised(item.player));
            let mob = summon(&mut a, from, event, DVec3::new(11.5, 64.0, 0.5), 90.0);
            let sword = converted.held.get("mainhand").cloned();
            if let Some(name) = &sword {
                let world = a.world();
                let stack = crate::server::game::world_stack(world, name, 1).unwrap();
                world.get_mut::<gear::MobEquipment>(mob).unwrap().set(
                    GearSlot::Mainhand,
                    stack,
                    false,
                );
            }
            let mut tick: u32 = 0;
            while a.world().get_entity(mob).is_ok() && tick < 1000 {
                a.ticks(1);
                tick += 1;
            }
            // The sweep sampled from about ten ticks after the summon.
            let bds = converted.first + 10;
            assert!(
                tick.abs_diff(bds) <= 2,
                "{}: ours {tick} vs BDS {bds}",
                item.key
            );
            let world = a.world();
            let mut query = world.query::<(Entity, &MobKind)>();
            let into: Vec<Entity> = query
                .iter(world)
                .filter(|(_, kind)| *kind.0.identifier == *converted.into)
                .map(|(entity, _)| entity)
                .collect();
            assert_eq!(into.len(), 1, "{}", item.key);
            if let Some(name) = sword {
                let held = gear::mainhand(world, into[0]).map(|stack| stack.item_id().to_owned());
                assert_eq!(held.as_deref(), Some(name.as_str()), "{}", item.key);
            }
        }
    }
}
