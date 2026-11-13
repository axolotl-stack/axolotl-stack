//! The bat replayed against BDS 1.26.51 (`mob/ai/testdata/
//! bds-1.26.51-bat.json`, distilled by `tools/bds-sweep/mob_fixture.py`
//! from `sweep.py bat`): its native AddActor, its float flight tick by
//! tick, its wander points and durations by distribution, and its resting.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use serde::Deserialize;

use super::drowned::DY;
use super::facets::{Arena, arena, arena_of, block, floor};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, OnGround, Position, Velocity};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::ai::roost::roofed;
use crate::entity::mob::components::FloatWanderGoal;
use crate::world::rules::Difficulty;

const BAT: &str = "minecraft:bat";
/// A float-navigating flyer's air drag on x and z, and the bat's on y
/// (0.91 × 0.6).
const DRAG: DVec3 = DVec3::new(0.91, 0.546, 0.91);
/// Two pushes aim at one point when their directions agree this well: the
/// fixture keeps four decimals, which blurs a direction by up to about a
/// degree.
const AIMED: f64 = 0.9995;

#[derive(Deserialize)]
struct Fixture {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    key: String,
    /// `[t, x, y, z]` every tick the player's feet moved.
    player_ticks: Vec<[f64; 4]>,
    actors: Vec<Actor>,
}

#[derive(Deserialize)]
struct Actor {
    /// `[t, x, y, z, yaw, pitch, vx, vy, vz, ground, health]`.
    ticks: Vec<[f64; 11]>,
    flag_changes: Vec<FlagChange>,
}

#[derive(Deserialize)]
struct FlagChange {
    t: u32,
    flag: u32,
    set: bool,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-bat.json"))
        .expect("bat fixture decodes")
}

impl Fixture {
    fn items<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items
            .iter()
            .filter(move |item| item.key.starts_with(kind))
    }
}

impl Actor {
    fn feet(&self, i: usize) -> DVec3 {
        let row = &self.ticks[i];
        DVec3::new(row[1], row[2], row[3])
    }

    /// The velocity BDS reports on sample `i`: the move of that tick.
    fn velocity(&self, i: usize) -> DVec3 {
        let row = &self.ticks[i];
        DVec3::new(row[6], row[7], row[8])
    }

    fn ground(&self, i: usize) -> bool {
        self.ticks[i][9] != 0.0
    }

    /// The ticks `RESTING` was raised (`true`) or lowered.
    fn rests(&self) -> Vec<(u32, bool)> {
        self.flag_changes
            .iter()
            .filter(|change| change.flag == actor_flags::RESTING)
            .map(|change| (change.t, change.set))
            .collect()
    }
}

/// The unit push of BDS's sample `i` (`None` on a tick it did not push, or
/// whose move a block or the floor cut).
fn push(actor: &Actor, i: usize) -> Option<DVec3> {
    if actor.ground(i) || actor.ground(i - 1) {
        return None;
    }
    let (v, w) = (actor.velocity(i - 1), actor.velocity(i));
    // A cut move does not match its reported velocity.
    if (actor.feet(i) - actor.feet(i - 1) - w).length() > 1.0e-3 {
        return None;
    }
    let push = w - v * DRAG;
    (push.length() > 0.05).then(|| push.normalize())
}

/// The point BDS's pushes on samples `i` and `i + 1` both aim at, from the
/// feet before each move, if they aim at one.
fn shared_point(actor: &Actor, i: usize) -> Option<DVec3> {
    let (a, b) = (push(actor, i)?, push(actor, i + 1)?);
    let (pa, pb) = (actor.feet(i - 1), actor.feet(i));
    let cos = a.dot(b);
    if cos > 0.999_999 {
        return None;
    }
    let w = pa - pb;
    let (d, e) = (a.dot(w), b.dot(w));
    let s = (cos * e - d) / (1.0 - cos * cos);
    let t = (e - cos * d) / (1.0 - cos * cos);
    let gap = (w + a * s - b * t).length();
    (gap < 0.02 && s > 0.0 && t > 0.0).then(|| pa + a * s)
}

/// One tick of a trace as the wander reader sees it.
enum Step {
    /// A block or the floor cut the move: the reading restarts.
    Cut,
    /// No push.
    Idle,
    /// A push in this direction.
    Push(DVec3),
}

fn step(actor: &Actor, i: usize) -> Step {
    let cut = actor.ground(i)
        || actor.ground(i - 1)
        || (actor.feet(i) - actor.feet(i - 1) - actor.velocity(i)).length() > 1.0e-3;
    if cut {
        return Step::Cut;
    }
    push(actor, i).map_or(Step::Idle, Step::Push)
}

/// A wander being read: its point, pushes, the idle ticks before it, and
/// whether it is counted.
struct Run {
    point: Option<DVec3>,
    pushes: u32,
    idle_before: u32,
    counted: bool,
}

/// Wander lengths in ticks (the pushes at one point plus the idle ticks
/// before them), counted for the wanders that follow another wander or an
/// idle tick and end on an idle tick or the next wander, and how many of
/// them began with an idle tick: BDS's trace or ours, as feet and the move
/// of each tick.
fn wander_lengths(actor: &Actor) -> (BTreeMap<u32, u32>, u32) {
    let mut lengths = BTreeMap::new();
    let mut idle_starts = 0;
    let mut record = |run: Run| {
        if run.counted {
            *lengths.entry(run.pushes + run.idle_before).or_insert(0) += 1;
            idle_starts += u32::from(run.idle_before > 0);
        }
    };
    let mut run: Option<Run> = None;
    let mut idle = 0;
    // Whether the next wander starts after a clean boundary (not after a
    // cut, nor at the start of the trace).
    let mut clean = false;
    for i in 1..actor.ticks.len() - 1 {
        match step(actor, i) {
            Step::Cut => {
                run = None;
                idle = 0;
                clean = false;
            }
            Step::Idle => {
                if let Some(done) = run.take() {
                    record(done);
                    clean = true;
                }
                idle += 1;
            }
            Step::Push(direction) => {
                let feet = actor.feet(i - 1);
                if let Some(current) = run.as_mut()
                    && let Some(point) = current.point
                    && (point - feet).length() > 1.0e-6
                    && (point - feet).normalize().dot(direction) > AIMED
                {
                    current.pushes += 1;
                    continue;
                }
                if let Some(done) = run.take() {
                    record(done);
                    clean = true;
                }
                let point = shared_point(actor, i);
                run = Some(Run {
                    point,
                    pushes: 1,
                    idle_before: idle,
                    counted: clean && point.is_some(),
                });
                idle = 0;
            }
        }
    }
    (lengths, idle_starts)
}

#[test]
fn spawned_bat_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/bat.bin"
    ));
    let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let bat = a.adult(BAT, feet);
    // Born resting, with CANFLY from `can_fly` and HAS_GRAVITY from its
    // physics, though it never falls.
    assert_add_actor_matches(a.world(), BAT, bat, &native);
}

/// A bat over the floor of the arena (BDS's superflat floor is 124 blocks
/// lower), its goals left to run unless a test pins its wander point.
fn bat_at(a: &mut Arena, feet: DVec3) -> Entity {
    a.adult(BAT, feet)
}

/// BDS's open-sky bats, tick by tick: placed where BDS's bat stood before
/// a wander, with its motion after that tick's drag, and flying at the
/// point BDS's pushes aim at, the bat moves like BDS's for four ticks,
/// within three thousandths of a block (the fixture's four decimals blur
/// the point), wander after wander (`v = drag × v + 0.1 × the unit vector
/// from the feet to the point`, no gravity).
#[test]
fn a_flying_bat_pushes_at_its_point_like_bds() {
    let fixture = fixture();
    let mut replayed = 0;
    let mut a = floor(2, Difficulty::Normal, GameMode::Creative);
    for item in fixture.items("open") {
        let bds = &item.actors[0];
        let mut i = 2;
        while i + 6 < bds.ticks.len() {
            // The first four pushes of a wander: all at the point its first
            // two aim at, where the tick before aimed elsewhere.
            let Some(point) = shared_point(bds, i) else {
                i += 1;
                continue;
            };
            let aims = |k: usize| {
                push(bds, k).is_some_and(|d| (point - bds.feet(k - 1)).normalize().dot(d) > AIMED)
            };
            if !(i..i + 4).all(aims) || aims(i - 1) {
                i += 1;
                continue;
            }
            // Moved into the arena's middle, so its floor and edges stay far.
            let shift = DVec3::new(8.0, DY, 8.0) - bds.feet(i - 1) * DVec3::new(1.0, 0.0, 1.0);
            let bat = bat_at(&mut a, bds.feet(i - 1) + shift);
            let world = a.world();
            set_flag(world, bat, actor_flags::RESTING, false);
            world.get_mut::<Velocity>(bat).unwrap().0 = bds.velocity(i - 1) * DRAG;
            let mut previous = bds.feet(i - 1) + shift;
            for k in i..i + 4 {
                let world = a.world();
                world
                    .get_mut::<MobBrain>(bat)
                    .unwrap()
                    .fly_mut()
                    .set_wander(point + shift, 1);
                a.ticks(1);
                let now = a.feet(bat);
                let theirs = bds.feet(k) + shift;
                assert!(
                    (now - theirs).length() < 3.0e-3,
                    "{} t {k}: at {now} vs BDS {theirs} (from {previous})",
                    item.key
                );
                previous = now;
            }
            a.world().despawn(bat);
            replayed += 1;
            i += 4;
        }
    }
    assert!(replayed >= 60, "only {replayed} wanders replayed");
}

fn set_flag(world: &mut World, mob: Entity, flag: u32, on: bool) {
    world
        .get_mut::<ActorFlags>(mob)
        .unwrap()
        .update(flag, on)
        .unwrap();
}

/// Our bat wanders like BDS's under the open sky: wanders last 3 to 7
/// ticks, about equally often, and BDS's and ours begin with an idle tick
/// about equally often (a point in the floor is not kept). Our trace is
/// read with the same rules as BDS's.
#[test]
fn bat_wanders_match_bds_by_distribution() {
    let fixture = fixture();
    let mut bds = BTreeMap::new();
    let mut bds_idle = 0;
    for item in fixture.items("open") {
        let (lengths, idle) = wander_lengths(&item.actors[0]);
        for (length, n) in lengths {
            *bds.entry(length).or_insert(0) += n;
        }
        bds_idle += idle;
    }
    let mut ours = BTreeMap::new();
    let mut ours_idle = 0;
    for seed in 0..6 {
        // Wide enough that the bat stays over loaded chunks.
        let mut a = superflat(10 + seed);
        // BDS's open bats were summoned 8 blocks over the floor.
        let bat = bat_at(&mut a, DVec3::new(8.5, 72.0, 8.5));
        let mut trace = Vec::new();
        let mut previous = a.feet(bat);
        for t in 0..600 {
            a.ticks(1);
            let world = a.world();
            let feet = world.get::<Position>(bat).unwrap().0;
            let on_ground = world.get::<OnGround>(bat).is_some_and(|g| g.0);
            let moved = feet - previous;
            previous = feet;
            trace.push([
                f64::from(t),
                feet.x,
                feet.y,
                feet.z,
                0.0,
                0.0,
                moved.x,
                moved.y,
                moved.z,
                f64::from(u8::from(on_ground)),
                6.0,
            ]);
        }
        let (lengths, idle) = wander_lengths(&Actor {
            ticks: trace,
            flag_changes: Vec::new(),
        });
        for (length, n) in lengths {
            *ours.entry(length).or_insert(0) += n;
        }
        ours_idle += idle;
    }
    let share = |counts: &BTreeMap<u32, u32>| {
        let total: u32 = counts.values().sum();
        let in_range: u32 = (3..=7).map(|n| counts.get(&n).copied().unwrap_or(0)).sum();
        (total, f64::from(in_range) / f64::from(total))
    };
    let (bds_total, bds_in) = share(&bds);
    let (ours_total, ours_in) = share(&ours);
    assert!(bds_total > 120 && ours_total > 300, "{bds:?} / {ours:?}");
    assert!(bds_in > 0.9 && ours_in > 0.9, "{bds:?} / {ours:?}");
    for length in 3..=7 {
        let part = |counts: &BTreeMap<u32, u32>, total: u32| {
            f64::from(counts.get(&length).copied().unwrap_or(0)) / f64::from(total)
        };
        let (theirs, mine) = (part(&bds, bds_total), part(&ours, ours_total));
        assert!(
            (theirs - mine).abs() < 0.08,
            "{length} ticks: BDS {theirs:.3} vs ours {mine:.3} ({bds:?} / {ours:?})"
        );
    }
    let idle_share = |idle: u32, total: u32| f64::from(idle) / f64::from(total);
    let (theirs, mine) = (
        idle_share(bds_idle, bds_total),
        idle_share(ours_idle, ours_total),
    );
    assert!(
        (theirs - mine).abs() < 0.1,
        "idle starts: BDS {theirs:.3} vs ours {mine:.3}"
    );
}

/// The wander draws: within `±xz_dist` horizontally and `y_offset ±
/// y_dist` vertically of the feet, and 3 to 7 ticks long for the bat's
/// `[0.1, 0.35]` seconds.
#[test]
fn float_wander_draws_stay_in_the_authored_box() {
    let mut a = floor(3, Difficulty::Normal, GameMode::Creative);
    let bat = bat_at(&mut a, DVec3::new(8.5, 72.0, 8.5));
    let goal = *a.world().get::<FloatWanderGoal>(bat).unwrap();
    assert_eq!(
        (goal.xz_dist, goal.y_dist, goal.y_offset),
        (10.0, 7.0, -2.0)
    );
    assert_eq!(goal.duration(0.0), 3);
    assert_eq!(goal.duration(0.999), 7);
    let mut seen = 0;
    let mut last = None;
    for _ in 0..400 {
        let feet = a.feet(bat);
        a.ticks(1);
        let world = a.world();
        let point = world.get::<MobBrain>(bat).unwrap().fly().point();
        if let Some(point) = point
            && last != Some(point)
        {
            let offset = point - feet;
            assert!(offset.x.abs() <= 10.0 && offset.z.abs() <= 10.0, "{offset}");
            assert!((-9.0..=5.0).contains(&offset.y), "{offset}");
            seen += 1;
        }
        last = point;
    }
    assert!(seen > 40, "{seen} points");
}

/// BDS's superflat floor, four blocks thick (y -64..-61, raised by
/// [`DY`]): a wander point inside it is not kept. Wide enough that a
/// wandering bat stays over loaded chunks.
fn superflat(seed: u64) -> Arena {
    let dirt = block("minecraft:dirt");
    arena_of(
        seed,
        Difficulty::Normal,
        GameMode::Creative,
        4,
        move |pos: IVec3| (-64..=-62).contains(&(pos.y - DY as i32)).then_some(dirt),
    )
}

/// The sweep's one-block room (`sweep.py`'s `LOW`, raised by [`DY`]):
/// stone x 10..18, y -61..-59, z -4..4, hollow.
fn low_room(seed: u64) -> Arena {
    let stone = block("minecraft:stone");
    arena(
        seed,
        Difficulty::Normal,
        GameMode::Creative,
        move |pos: IVec3| {
            let (x, y, z) = (pos.x, pos.y - DY as i32, pos.z);
            let inside = (11..=17).contains(&x) && y == -60 && (-3..=3).contains(&z);
            let shell = (10..=18).contains(&x) && (-61..=-59).contains(&y) && (-4..=4).contains(&z);
            (shell && !inside).then_some(stone)
        },
    )
}

/// `/damage @e[type=minecraft:bat] 1`: a hit with no attacker.
fn hit(damage: f32) -> crate::entity::mob::combat::hit::Hit {
    crate::entity::mob::combat::hit::Hit {
        damage,
        cause: unastar_data::actor_components::body::ActorDamageCause::None,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
    }
}

fn resting(world: &World, bat: Entity) -> bool {
    world
        .get::<ActorFlags>(bat)
        .unwrap()
        .get(actor_flags::RESTING)
        .unwrap()
}

/// A bat is born resting. In the open it wakes on its first tick; summoned
/// under a roof it stays, its head against the roof, and does not move
/// (BDS 1.26.51: every summoned bat of the cave sweeps woke before the
/// first sample; every one in the one-block room rested, at y -59.9, until
/// it was hurt).
#[test]
fn a_bat_is_born_resting_and_wakes_without_a_roof() {
    let mut a = floor(4, Difficulty::Normal, GameMode::Creative);
    let bat = bat_at(&mut a, DVec3::new(8.5, 70.0, 8.5));
    assert!(resting(a.world(), bat));
    a.ticks(1);
    assert!(!resting(a.world(), bat));

    let mut a = low_room(5);
    let bat = bat_at(&mut a, DVec3::new(14.5, -60.0 + DY, 0.5));
    let world = a.world();
    assert!(roofed(&*world, DVec3::new(14.5, -60.0 + DY, 0.5)));
    for _ in 0..100 {
        a.ticks(1);
        assert!(resting(a.world(), bat));
    }
    let feet = a.feet(bat);
    assert!(
        (feet - DVec3::new(14.5, -59.9 + DY, 0.5)).length() < 1.0e-6,
        "{feet}"
    );
}

/// A resting bat wakes on the tick it is hurt and on the tick its roof is
/// removed, and a player's eyes within 4 blocks of its feet wake it
/// (BDS 1.26.51, `sweep.py bat --items low,unroof,canopy`).
#[test]
fn a_resting_bat_wakes_when_hurt_unroofed_or_approached() {
    use crate::entity::mob::combat::hit::hurt_mob;
    let mut a = low_room(6);
    let feet = DVec3::new(14.5, -60.0 + DY, 0.5);
    let bat = bat_at(&mut a, feet);
    a.ticks(5);
    assert!(resting(a.world(), bat));
    hurt_mob(a.world(), bat, hit(1.0));
    assert!(!resting(a.world(), bat));

    let mut a = low_room(7);
    let bat = bat_at(&mut a, feet);
    a.ticks(5);
    a.set_block(IVec3::new(14, -59 + DY as i32, 0), "minecraft:air");
    a.ticks(1);
    assert!(!resting(a.world(), bat));

    let fixture = fixture();
    for item in fixture.items("canopy") {
        let player = item.player_ticks.last().unwrap();
        let bds = &item.actors[0];
        let woke = bds.rests().contains(&(40, false));
        let mut a = canopy(8);
        let bat = bat_at(&mut a, DVec3::new(14.5, -59.0 + DY, 0.5));
        a.ticks(20);
        assert!(resting(a.world(), bat), "{}", item.key);
        a.move_player(DVec3::new(player[1], player[2] + DY, player[3]));
        a.ticks(1);
        assert_eq!(!resting(a.world(), bat), woke, "{}", item.key);
    }
}

/// The sweep's canopy (`sweep.py`'s `CANOPY`, raised by [`DY`]): stone at
/// y -58, x 8..20, z -3..3.
fn canopy(seed: u64) -> Arena {
    let stone = block("minecraft:stone");
    arena(
        seed,
        Difficulty::Normal,
        GameMode::Survival,
        move |pos: IVec3| {
            let (x, y, z) = (pos.x, pos.y - DY as i32, pos.z);
            ((8..=20).contains(&x) && y == -58 && (-3..=3).contains(&z)).then_some(stone)
        },
    )
}

/// How long a bat that was woken flies before it settles again, BDS's and
/// ours, in the one-block room (always under the roof): both fit a chance
/// near 1 in 100 per tick. Ours is hurt on the same ticks as BDS's.
#[test]
fn bat_settling_matches_bds_by_distribution() {
    use crate::entity::mob::combat::hit::hurt_mob;
    let fixture = fixture();
    // (ticks flown before settling, settled at all)
    let flights = |rests: &[(u32, bool)], end: u32| {
        let mut out = Vec::new();
        let mut woke = None;
        for &(t, set) in rests {
            match (set, woke) {
                (false, _) => woke = Some(t),
                (true, Some(from)) => {
                    out.push((t - from, true));
                    woke = None;
                }
                (true, None) => {}
            }
        }
        if let Some(from) = woke {
            out.push((end - from, false));
        }
        out
    };
    let rate = |flights: &[(u32, bool)]| {
        let settled = flights.iter().filter(|(_, settled)| *settled).count();
        let exposure: u32 = flights.iter().map(|(ticks, _)| ticks).sum();
        settled as f64 / f64::from(exposure)
    };
    let mut bds = Vec::new();
    for item in fixture.items("low") {
        bds.extend(flights(&item.actors[0].rests(), 400));
    }
    let mut ours = Vec::new();
    for seed in 0..24 {
        let mut a = low_room(20 + seed);
        let bat = bat_at(&mut a, DVec3::new(14.5, -60.0 + DY, 0.5));
        let mut rests = Vec::new();
        let mut was = true;
        for t in 0..400u32 {
            if [20, 120, 220, 320].contains(&t) {
                hurt_mob(a.world(), bat, hit(1.0));
            }
            let now = resting(a.world(), bat);
            if now != was {
                rests.push((t, now));
                was = now;
            }
            a.ticks(1);
            let now = resting(a.world(), bat);
            if now != was {
                rests.push((t + 1, now));
                was = now;
            }
        }
        ours.extend(flights(&rests, 400));
    }
    let (theirs, mine) = (rate(&bds), rate(&ours));
    assert!(bds.len() >= 20 && ours.len() >= 60);
    assert!(
        (theirs - mine).abs() / theirs < 0.35,
        "settling rate: BDS {theirs:.4} vs ours {mine:.4}"
    );
}
