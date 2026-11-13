//! The vex replayed against BDS 1.26.51 (`mob/ai/testdata/
//! bds-1.26.51-vex.json`, distilled by `tools/bds-sweep/mob_fixture.py`
//! from `sweep.py vex`): its native AddActor, its flight through its move
//! control tick by tick, its wanders by distribution, its charge and hit,
//! and its decay.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use serde::Deserialize;

use super::facets::{Arena, floor};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, Health, Position, Rotation, Velocity};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::ai::fly::Course;
use crate::entity::mob::ai::vex::arrival_distance;
use crate::entity::mob::constants::{
    VEX_ARRIVAL_SLOWDOWN, VEX_CHARGE_AIM_DROP, VEX_PUSH_SCALE, VEX_WANDER_SPEED,
};
use crate::world::rules::Difficulty;

const VEX: &str = "minecraft:vex";
/// The vex's drag on every axis.
const DRAG: f64 = 0.91;
/// The vex's box (`minecraft:collision_box`).
const WIDTH: f64 = 0.4;
const HEIGHT: f64 = 0.8;
/// A player's eyes above its feet.
const PLAYER_EYES: f64 = crate::entity::PLAYER_EYE_HEIGHT;

#[derive(Deserialize)]
struct Fixture {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    key: String,
    /// `[t, x, y, z]` every tick the player's feet moved.
    player_ticks: Vec<[f64; 4]>,
    damage: Vec<Damage>,
    actors: Vec<Actor>,
}

#[derive(Deserialize)]
struct Damage {
    t: u32,
    #[serde(rename = "type")]
    kind: String,
    amount: f32,
    cause: String,
}

#[derive(Deserialize)]
struct Actor {
    #[serde(rename = "type")]
    kind: String,
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
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-vex.json"))
        .expect("vex fixture decodes")
}

impl Fixture {
    fn items<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Item> + 'a {
        self.items
            .iter()
            .filter(move |item| item.key.starts_with(kind))
    }
}

impl Item {
    fn vex(&self) -> &Actor {
        self.actors
            .iter()
            .find(|actor| actor.kind == VEX)
            .expect("the item tracks its vex")
    }

    /// The player's feet on sample `t`.
    fn player(&self, t: u32) -> DVec3 {
        let row = self
            .player_ticks
            .iter()
            .rev()
            .find(|row| row[0] as u32 <= t)
            .unwrap_or(&self.player_ticks[0]);
        DVec3::new(row[1], row[2], row[3])
    }
}

impl Actor {
    fn feet(&self, i: usize) -> DVec3 {
        let row = &self.ticks[i];
        DVec3::new(row[1], row[2], row[3])
    }

    fn yaw(&self, i: usize) -> f32 {
        self.ticks[i][4] as f32
    }

    /// The velocity BDS reports on sample `i`: the move of the tick before.
    fn velocity(&self, i: usize) -> DVec3 {
        let row = &self.ticks[i];
        DVec3::new(row[6], row[7], row[8])
    }

    /// The push of sample `i`: its move less the dragged move before it.
    fn push(&self, i: usize) -> DVec3 {
        self.velocity(i) - self.velocity(i - 1) * DRAG
    }

    /// Whether the move of sample `i` kept the arrival share of the one
    /// before, dragged.
    fn arrived(&self, i: usize) -> bool {
        let (before, now) = (self.velocity(i - 1).length(), self.velocity(i).length());
        before > 0.01 && (now / before - VEX_ARRIVAL_SLOWDOWN * DRAG).abs() < 0.01
    }

    fn flag_ticks(&self, flag: u32, set: bool) -> Vec<u32> {
        self.flag_changes
            .iter()
            .filter(|change| change.flag == flag && change.set == set)
            .map(|change| change.t)
            .collect()
    }
}

/// The block centre a wander from sample `i` to its arrival on sample
/// `arrival` flew at: the centre nearest the feet the arrival was judged
/// from, if the first push aims at it.
fn wander_point(actor: &Actor, i: usize, arrival: usize) -> Option<DVec3> {
    let point = actor.feet(arrival - 1).floor() + DVec3::splat(0.5);
    let aim = (point - actor.feet(i - 1)).normalize();
    (aim.dot(actor.push(i).normalize()) > 0.999).then_some(point)
}

/// A vex in the air over the arena, its goals left to run.
fn vex_at(a: &mut Arena, feet: DVec3) -> Entity {
    a.adult(VEX, feet)
}

fn set_course(world: &mut World, vex: Entity, point: DVec3, push: f64) {
    world
        .get_mut::<MobBrain>(vex)
        .unwrap()
        .fly_mut()
        .set_course(Course { point, push });
}

#[test]
fn spawned_vex_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/vex.bin"
    ));
    let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let vex = a.adult(VEX, feet);
    assert_add_actor_matches(a.world(), VEX, vex, &native);
}

/// Replays BDS's flight from sample `i - 1`, its course `point` at `push`
/// a tick, for `ticks` ticks: the feet within a thousandth of a block every
/// tick, and with `yaw` the body yaw within a tenth of a degree. BDS samples
/// a tick after its AI and before its move: the yaw of sample `k - 1` was
/// set in the tick that made the move of sample `k`.
#[allow(clippy::too_many_arguments)]
fn replay(
    a: &mut Arena,
    key: &str,
    bds: &Actor,
    i: usize,
    point: DVec3,
    push: f64,
    ticks: usize,
    yaw: bool,
) {
    // Moved over the arena's middle (the vex passes through blocks).
    let shift = DVec3::new(8.0, 70.0, 8.0) - bds.feet(i - 1);
    let vex = vex_at(a, bds.feet(i - 1) + shift);
    let world = a.world();
    world.get_mut::<Velocity>(vex).unwrap().0 = bds.velocity(i - 1) * DRAG;
    world.get_mut::<Rotation>(vex).unwrap().yaw = bds.yaw(i - 2);
    set_course(world, vex, point + shift, push);
    for k in i..i + ticks {
        a.ticks(1);
        let world = a.world();
        let now = world.get::<Position>(vex).unwrap().0;
        let theirs = bds.feet(k) + shift;
        assert!(
            (now - theirs).length() < 1.0e-3,
            "{key} t {k}: at {now} vs BDS {theirs}"
        );
        if yaw {
            let ours = world.get::<Rotation>(vex).unwrap().yaw;
            let off = ((ours - bds.yaw(k - 1) + 540.0).rem_euclid(360.0) - 180.0).abs();
            // The fixture's four decimals blur the direction of a slow move.
            let moved = bds.velocity(k);
            let blur = (1.0e-4 / moved.x.hypot(moved.z)).atan().to_degrees() as f32;
            assert!(
                off < 0.1 + blur,
                "{key} t {k}: yaw {ours} vs BDS {}",
                bds.yaw(k - 1)
            );
        }
    }
    a.world().despawn(vex);
}

/// BDS's wandering vexes, tick by tick: placed where BDS's vex stood before
/// a wander's first push, with its motion after that tick's drag, and
/// flying at the block centre BDS's pushes aim at, the vex pushes 0.0125 a
/// tick at it, turns its body toward its motion at most 30° a tick, and
/// keeps half its velocity on the tick its feet come within its box size
/// of the point, like BDS's, to the end of the wander.
#[test]
fn a_wandering_vex_flies_its_course_like_bds() {
    let fixture = fixture();
    let push = VEX_WANDER_SPEED * VEX_PUSH_SCALE;
    let mut a = floor(2, Difficulty::Normal, GameMode::Creative);
    let mut replayed = 0;
    for item in fixture.items("idle").chain(fixture.items("look")) {
        let bds = item.vex();
        for i in 2..bds.ticks.len() - 1 {
            // A wander's first push: the push before it was none, a slow
            // down, or one in another direction.
            let pushes = |k: usize| (bds.push(k).length() - push).abs() < 5.0e-4;
            let first = pushes(i)
                && !(pushes(i - 1)
                    && bds.push(i - 1).normalize().dot(bds.push(i).normalize()) > 0.99);
            if !first {
                continue;
            }
            let Some(arrival) = (i + 1..bds.ticks.len()).find(|&k| bds.arrived(k)) else {
                continue;
            };
            let Some(point) = wander_point(bds, i, arrival) else {
                continue;
            };
            replay(
                &mut a,
                &item.key,
                bds,
                i,
                point,
                push,
                arrival + 1 - i,
                true,
            );
            replayed += 1;
        }
    }
    assert!(replayed >= 12, "only {replayed} wanders replayed");
}

/// The ticks between an arrival and the next push, for BDS's trace or
/// ours: one more than the 1-in-7 draws, as a course is taken up on the
/// tick after a goal sets it.
fn waits(ticks: &[[f64; 11]]) -> Vec<u32> {
    let actor = Actor {
        kind: VEX.into(),
        ticks: ticks.to_vec(),
        flag_changes: Vec::new(),
    };
    let mut out = Vec::new();
    let mut arrived = None;
    for i in 2..actor.ticks.len() {
        if actor.arrived(i) {
            arrived = Some(i);
        } else if let Some(at) = arrived
            && actor.push(i).length() > 0.008
        {
            out.push((i - at) as u32);
            arrived = None;
        }
    }
    out
}

/// Our vex wanders like BDS's: its waits after an arrival average about
/// 7.6 samples like the BDS vexes', and every
/// point is a block centre within 7 blocks of its block on x and z and 5 on
/// y.
#[test]
fn vex_wanders_match_bds_by_distribution() {
    let fixture = fixture();
    let bds: Vec<u32> = fixture
        .items("idle")
        .chain(fixture.items("look"))
        .flat_map(|item| waits(&item.vex().ticks))
        .collect();
    let mut ours = Vec::new();
    let mut points = 0;
    for seed in 0..4 {
        let mut a = floor(20 + seed, Difficulty::Normal, GameMode::Creative);
        let vex = vex_at(&mut a, DVec3::new(8.5, 72.0, 8.5));
        let mut trace = Vec::new();
        let mut previous = a.feet(vex);
        let mut last_point = None;
        for t in 0..1200 {
            let feet = a.feet(vex);
            a.ticks(1);
            let world = a.world();
            let now = world.get::<Position>(vex).unwrap().0;
            let course = world.get::<MobBrain>(vex).unwrap().fly().course();
            if let Some(course) = course
                && last_point != Some(course.point)
            {
                let offset = course.point - (feet.floor() + DVec3::splat(0.5));
                assert!(
                    offset.x.abs() <= 7.0 && offset.y.abs() <= 5.0 && offset.z.abs() <= 7.0,
                    "{offset}"
                );
                let corner = course.point - DVec3::splat(0.5);
                assert_eq!(corner, corner.floor());
                last_point = Some(course.point);
                points += 1;
            }
            let moved = now - previous;
            previous = now;
            trace.push([
                f64::from(t),
                now.x,
                now.y,
                now.z,
                0.0,
                0.0,
                moved.x,
                moved.y,
                moved.z,
                0.0,
                14.0,
            ]);
        }
        ours.extend(waits(&trace));
    }
    let mean = |waits: &[u32]| waits.iter().sum::<u32>() as f64 / waits.len() as f64;
    assert!(
        bds.len() >= 10 && ours.len() >= 60,
        "{bds:?} / {}",
        ours.len()
    );
    assert!(points >= 60, "{points}");
    assert!(ours.iter().all(|&w| w >= 1), "{ours:?}");
    let (theirs, mine) = (mean(&bds), mean(&ours));
    assert!(
        (theirs - mine).abs() < 1.5,
        "mean wait: BDS {theirs:.2} vs ours {mine:.2}"
    );
}

/// BDS's charges, tick by tick: from the tick before a charge's first push,
/// the vex flies at 0.05 a tick at 0.1 below the player's eyes, and slows
/// down when it arrives, like BDS's.
#[test]
fn a_charging_vex_flies_at_its_target_like_bds() {
    let fixture = fixture();
    let push = VEX_PUSH_SCALE;
    let mut a = floor(3, Difficulty::Normal, GameMode::Creative);
    let mut replayed = 0;
    for item in fixture.items("charge") {
        let bds = item.vex();
        for start in bds.flag_ticks(actor_flags::CHARGING, true) {
            let i = start as usize + 2;
            assert!(
                (bds.push(i).length() - push).abs() < 5.0e-4,
                "{} t {start}: the first push comes two samples after the charge starts",
                item.key
            );
            let Some(arrival) = (i + 1..bds.ticks.len()).find(|&k| bds.arrived(k)) else {
                continue;
            };
            // The player stands still until the hit knocks it away.
            let point = item.player(start) + DVec3::Y * (PLAYER_EYES - VEX_CHARGE_AIM_DROP);
            // BDS's vex faced its target; ours has none here.
            replay(
                &mut a,
                &item.key,
                bds,
                i,
                point,
                push,
                arrival + 1 - i,
                false,
            );
            replayed += 1;
        }
    }
    assert!(replayed >= 8, "only {replayed} charges replayed");
}

/// In the running game, a vex charges a survival player: `CHARGING` goes
/// up, the first push comes two ticks later, the hit lands on the tick of
/// the move that brought the two boxes together (BDS 1.26.51: every hit of
/// `sweep.py vex --items charge` on that tick) for 9 (its `attack` 3 and the
/// iron sword `vex_gear` gives it), and `CHARGING` goes down with it.
#[test]
fn a_vex_charges_and_hits_a_survival_player() {
    let mut a = floor(4, Difficulty::Normal, GameMode::Survival);
    let player_feet = DVec3::new(12.5, 64.0, 12.5);
    let vex = vex_at(&mut a, player_feet + DVec3::new(8.0, 2.0, 0.0));
    let player = a.player;
    let health = a.world().get::<Health>(player).unwrap().current;
    let mut start = None;
    let mut first_push = None;
    let mut hit = None;
    let mut previous_velocity = DVec3::ZERO;
    for t in 0..400u32 {
        a.ticks(1);
        let world = a.world();
        let charging = world
            .get::<ActorFlags>(vex)
            .unwrap()
            .get(actor_flags::CHARGING)
            .unwrap();
        let velocity = world.get::<Velocity>(vex).unwrap().0;
        if charging && start.is_none() {
            start = Some(t);
        }
        // Our velocity after tick t is the move of tick t + 1, dragged.
        if start.is_some()
            && first_push.is_none()
            && (velocity - previous_velocity * DRAG).length() > 0.04
        {
            first_push = Some(t);
        }
        previous_velocity = velocity;
        let now = world.get::<Health>(player).unwrap().current;
        if now < health {
            assert_eq!(health - now, 9.0);
            assert!(!charging, "the charge ends with the hit");
            let own = crate::registry::collision::Aabb::from_feet(
                world.get::<Position>(vex).unwrap().0,
                WIDTH,
                HEIGHT,
            );
            let theirs = crate::registry::collision::Aabb::from_feet(player_feet, 0.6, 1.8);
            assert!(own.intersects(&theirs), "hit after the move that touched");
            hit = Some(t);
            break;
        }
    }
    let target = a
        .world()
        .get::<crate::entity::mob::state::AttackTarget>(vex)
        .map(|t| t.0);
    let (start, first_push, hit) = (
        start.unwrap_or_else(|| panic!("no charge (target {target:?})")),
        first_push.expect("a push"),
        hit.unwrap_or_else(|| panic!("no hit (charge from {start:?}, push {first_push:?})")),
    );
    // The goal sets the course on `start`; the control first pushes in the
    // tick after, whose velocity we read after it.
    assert_eq!(first_push, start + 1);
    assert!(hit > first_push);
}

/// BDS's hits came on the tick of the move that brought the vex's box into
/// the player's: the vex's feet after that tick's move (sample t + 1)
/// touch, those before it (sample t) do not.
#[test]
fn bds_charge_hits_come_on_the_move_that_touches() {
    let fixture = fixture();
    let mut hits = 0;
    for item in fixture.items("charge") {
        let bds = item.vex();
        for damage in &item.damage {
            assert_eq!(
                (damage.kind.as_str(), damage.amount, damage.cause.as_str()),
                ("minecraft:player", 9.0, "entity_attack")
            );
            let t = damage.t as usize;
            let player =
                crate::registry::collision::Aabb::from_feet(item.player(damage.t), 0.6, 1.8);
            let own =
                |i: usize| crate::registry::collision::Aabb::from_feet(bds.feet(i), WIDTH, HEIGHT);
            assert!(
                !own(t).intersects(&player) && own(t + 1).intersects(&player),
                "{} t {t}",
                item.key
            );
            hits += 1;
        }
    }
    assert!(hits >= 10, "{hits}");
}

/// The decay: once `minecraft:add_periodic_damage` adds the periodic damage
/// group, the vex hurts itself by 1 with no cause every 20 ticks, the first
/// 20 ticks after, and dies on its 14th hurt, like both BDS decays.
#[test]
fn a_decaying_vex_hurts_every_second_until_it_dies() {
    let fixture = fixture();
    for item in fixture.items("decay") {
        let hurts: Vec<u32> = item.damage.iter().map(|d| d.t).collect();
        assert_eq!(hurts.len(), 14, "{}", item.key);
        assert!(hurts.windows(2).all(|w| w[1] - w[0] == 20), "{hurts:?}");
        assert!(
            item.damage
                .iter()
                .all(|d| d.amount == 1.0 && d.cause == "none")
        );
    }
    let mut a = floor(5, Difficulty::Normal, GameMode::Creative);
    let vex = vex_at(&mut a, DVec3::new(8.5, 72.0, 8.5));
    let world = a.world();
    let event = world
        .get::<MobKind>(vex)
        .unwrap()
        .0
        .events
        .event("minecraft:add_periodic_damage")
        .unwrap();
    crate::entity::mob::events::fire_event(world, vex, &event, None).unwrap();
    apply_pending_definitions(world);
    let mut hurts = Vec::new();
    let mut health = 14.0;
    for t in 1..=300u32 {
        a.ticks(1);
        let Some(now) = a.world().get::<Health>(vex).map(|h| h.current) else {
            break;
        };
        if now < health {
            hurts.push((t, health - now));
            health = now;
        }
        if now <= 0.0 {
            break;
        }
    }
    let expected: Vec<(u32, f32)> = (1..=14).map(|n| (20 * n, 1.0)).collect();
    assert_eq!(hurts, expected);
}

/// `minecraft:add_damage_timer` starts the decay after 30 to 119 seconds:
/// BDS's two vexes took their first hurt 856 and 743 ticks after their
/// summon, one hurt interval after the timer ran out.
#[test]
fn the_decay_timer_starts_within_its_authored_range() {
    let fixture = fixture();
    for item in fixture.items("decay") {
        let first = item.damage[0].t;
        assert!((30 * 20 + 20..=119 * 20 + 20).contains(&first), "{first}");
    }
}

/// `arrival_distance` is the vex's box size, the mean of its dimensions.
#[test]
fn the_vex_arrives_within_its_box_size() {
    assert!((arrival_distance(WIDTH, HEIGHT) - 1.6 / 3.0).abs() < 1.0e-12);
}

/// A vex summoned with `minecraft:add_damage_timer` starts its decay
/// within its timer's 30 to 119 seconds: the first hurt comes one interval
/// after the timer runs out, as BDS's did.
#[test]
fn a_vex_with_a_damage_timer_starts_decaying_in_its_range() {
    let mut a = floor(6, Difficulty::Normal, GameMode::Creative);
    let vex = super::drowned::summon(
        &mut a,
        VEX,
        "minecraft:add_damage_timer",
        DVec3::new(8.5, 72.0, 8.5),
        0.0,
    );
    let mut first = None;
    for t in 1..=2500u32 {
        a.ticks(1);
        let world = a.world();
        world.get_mut::<MobBrain>(vex).unwrap().fly_mut().reset();
        world.get_mut::<Position>(vex).unwrap().0 = DVec3::new(8.5, 72.0, 8.5);
        if world.get::<Health>(vex).unwrap().current < 14.0 {
            first = Some(t);
            break;
        }
    }
    let first = first.expect("the vex decays");
    assert!((30 * 20 + 20..=119 * 20 + 20).contains(&first), "{first}");
}

/// `has_collision: false` moves the vex through blocks: flying at a point
/// outside a closed glass box, it leaves the box through its wall and never
/// stands on anything, as BDS's vex left its glass room.
#[test]
fn a_vex_flies_out_of_a_closed_glass_box() {
    let glass = super::facets::block("minecraft:glass");
    let mut a = super::facets::arena(7, Difficulty::Normal, GameMode::Creative, move |pos| {
        let inside =
            (5..=11).contains(&pos.x) && (67..=73).contains(&pos.y) && (5..=11).contains(&pos.z);
        let shell =
            (4..=12).contains(&pos.x) && (66..=74).contains(&pos.y) && (4..=12).contains(&pos.z);
        (shell && !inside).then_some(glass)
    });
    let vex = vex_at(&mut a, DVec3::new(8.5, 69.0, 8.5));
    let push = VEX_WANDER_SPEED * VEX_PUSH_SCALE;
    set_course(a.world(), vex, DVec3::new(20.5, 69.5, 8.5), push);
    for _ in 0..200 {
        a.ticks(1);
        let world = a.world();
        assert!(
            !world
                .get::<crate::entity::components::OnGround>(vex)
                .unwrap()
                .0
        );
        if world.get::<Position>(vex).unwrap().0.x > 13.0 {
            return;
        }
    }
    panic!("the vex stayed in the box at {}", a.feet(vex));
}
