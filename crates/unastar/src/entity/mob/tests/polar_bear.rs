//! The polar bear replayed against BDS 1.26.51 (`mob/ai/testdata/
//! bds-1.26.51-polar_bear.json`, distilled by `tools/bds-sweep/mob_fixture.py`
//! from `sweep.py polar-bear`): its native AddActor, the stomp goal's
//! rearing and hits, and its water drag.

use glam::DVec3;
use jolyne::valentine::actor_flags;
use serde::Deserialize;

use super::drowned::{DY, pool, summon};
use super::facets::floor;
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, Health, PhysicsBody, Velocity};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::StompClock;
use crate::entity::mob::components::{MeleeAttackGoal, RandomStrollGoal};
use crate::entity::mob::constants::PLAYER_BOX_WIDTH;
use crate::world::rules::Difficulty;

const POLAR_BEAR: &str = "minecraft:polar_bear";
/// The actor flag bit BDS raises while a polar bear rears up.
const STANDING: u32 = actor_flags::STANDING;

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
    ticks: Vec<[Option<f64>; 11]>,
    flag_changes: Vec<FlagChange>,
    events: Vec<ActorEvent>,
}

#[derive(Deserialize)]
struct FlagChange {
    t: u32,
    flag: u32,
    set: bool,
}

#[derive(Deserialize)]
struct ActorEvent {
    t: u32,
    event: String,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-polar_bear.json"))
        .expect("polar bear fixture decodes")
}

fn item<'a>(fixture: &'a Fixture, key: &str) -> &'a Item {
    fixture
        .items
        .iter()
        .find(|item| item.key == key)
        .unwrap_or_else(|| panic!("no item {key}"))
}

impl Actor {
    fn feet(&self, t: u32) -> Option<DVec3> {
        self.ticks
            .iter()
            .find(|row| row[0] == Some(f64::from(t)))
            .map(|row| DVec3::new(row[1].unwrap(), row[2].unwrap(), row[3].unwrap()))
    }

    fn velocity(&self, t: u32) -> DVec3 {
        let row = self
            .ticks
            .iter()
            .find(|row| row[0] == Some(f64::from(t)))
            .expect("sampled tick");
        DVec3::new(row[6].unwrap(), row[7].unwrap(), row[8].unwrap())
    }

    /// Rear-ups, stand-downs and hits, by tick.
    fn stomp_events(&self) -> Vec<(u32, &'static str)> {
        let mut out: Vec<(u32, &'static str)> = self
            .flag_changes
            .iter()
            .filter(|change| change.flag == STANDING)
            .map(|change| (change.t, if change.set { "up" } else { "down" }))
            .chain(
                self.events
                    .iter()
                    .filter(|event| event.event == "StartAttacking")
                    .map(|event| (event.t, "hit")),
            )
            .collect();
        out.sort();
        out
    }
}

impl Item {
    /// The player's feet on tick `t`.
    fn player(&self, t: u32) -> DVec3 {
        let row = self
            .player_ticks
            .iter()
            .rev()
            .find(|row| row[0] <= f64::from(t))
            .expect("player sampled from tick 0");
        DVec3::new(row[1], row[2], row[3])
    }
}

#[test]
fn spawned_adult_polar_bear_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/polar_bear.bin"
    ));
    let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let bear = a.adult(POLAR_BEAR, feet);
    assert_add_actor_matches(a.world(), POLAR_BEAR, bear, &native);
}

/// The stomp goal of a hostile adult, as the definition compiles it, and
/// the bear's width.
fn hostile_stomp() -> (MeleeAttackGoal, f64) {
    let mut a = floor(2, Difficulty::Normal, GameMode::Creative);
    let bear = summon(
        &mut a,
        POLAR_BEAR,
        "minecraft:on_anger",
        DVec3::new(4.5, 64.0, 4.5),
        0.0,
    );
    let world = a.world();
    let goal = world
        .get::<MeleeAttackGoal>(bear)
        .expect("a hostile polar bear has its stomp goal")
        .clone();
    (goal, world.get::<PhysicsBody>(bear).unwrap().width)
}

/// BDS 1.26.51's hostile polar bears rear up ten ticks before every hit,
/// and stand down without a hit when their cooldown runs out with the
/// target in the no-damage range but out of reach: fed the bear's feet
/// after each tick's move and the player's before it, the stomp clock
/// rears, stands down and hits on exactly BDS's ticks, from the first
/// stand-down or hit's cooldown on. (`baby-0` is left out: its adult's
/// cooldown restarted mid-run, at t 165, for a reason not measured.)
#[test]
fn the_stomp_clock_rears_and_hits_on_bds_ticks() {
    let fixture = fixture();
    let (goal, width) = hostile_stomp();
    let (reach_sq, no_damage_sq) = goal
        .reach
        .stomp_ranges(width, PLAYER_BOX_WIDTH)
        .expect("the polar bear's goal has the stomp reach");
    for key in ["attack-0", "attack-1", "attack-2", "baby-1", "baby-2"] {
        let item = item(&fixture, key);
        // The adult (the baby items summon it first).
        let bear = &item.actors[0];
        let bds = bear.stomp_events();
        let first = bds
            .iter()
            .find(|(_, what)| *what != "up")
            .expect("a stand-down or hit")
            .0;
        let start = first - goal.cooldown_ticks as u32;
        let last = bear.ticks.len() as u32 - 2;
        let mut clock = StompClock::new(goal.cooldown_ticks);
        let mut ours = Vec::new();
        for t in start..last {
            let (Some(feet), player) = (bear.feet(t + 1), item.player(t)) else {
                continue;
            };
            let step = clock.step(feet.distance_squared(player), reach_sq, no_damage_sq);
            if step.hit {
                ours.push((t, "hit"));
            }
            match step.standing {
                Some(true) => ours.push((t, "up")),
                Some(false) => ours.push((t, "down")),
                None => {}
            }
        }
        ours.sort();
        let bds: Vec<_> = bds
            .into_iter()
            .filter(|(t, _)| (start..last).contains(t))
            .collect();
        assert_eq!(ours, bds, "{key}");
        assert!(bds.iter().filter(|(_, what)| *what == "hit").count() >= 4);
    }
}

/// In the running game a hostile polar bear two blocks from its target
/// rears up, then stomps ten ticks later, and keeps one hit every twenty
/// ticks while the target stays in reach.
#[test]
fn a_hostile_polar_bear_rears_up_then_stomps_its_target() {
    let mut a = floor(3, Difficulty::Normal, GameMode::Survival);
    let player = a.player;
    let bear = summon(
        &mut a,
        POLAR_BEAR,
        "minecraft:on_anger",
        DVec3::new(14.5, 64.0, 12.5),
        90.0,
    );
    let world = a.world();
    world.get_mut::<Health>(player).unwrap().current = 1000.0;
    world.get_mut::<Health>(player).unwrap().max = 1000.0;
    assert!(crate::entity::mob::ai::provoke(world, bear, player));
    let mut stood = Vec::new();
    let mut hits = Vec::new();
    let mut health = 1000.0;
    let mut standing = false;
    for t in 0..80u32 {
        a.ticks(1);
        // The player is moved only by its client: keep it in place.
        a.move_player(DVec3::new(12.5, 64.0, 12.5));
        let world = a.world();
        let now = world
            .get::<ActorFlags>(bear)
            .unwrap()
            .get(STANDING)
            .unwrap();
        if now && !standing {
            stood.push(t);
        }
        standing = now;
        let current = world.get::<Health>(player).unwrap().current;
        if current < health {
            hits.push(t);
        }
        health = current;
    }
    assert!(hits.len() >= 3, "hits {hits:?}");
    for pair in hits.windows(2) {
        assert_eq!(pair[1] - pair[0], 20, "hits {hits:?}");
    }
    for hit in &hits {
        assert!(
            stood.contains(&(hit - 10)),
            "stood {stood:?}, hits {hits:?}"
        );
    }
}

/// `water_movement`'s `drag_factor` 0.98 is the bear's horizontal drag in
/// water (its vertical drag stays 0.8): knocked across the pool it keeps
/// 0.98 of its horizontal motion a tick, tick for tick with BDS. Of the
/// three runs only `swim-push-0` stays under water untouched for the
/// window: in `swim-push-1` the bear was already walking when hit, and in
/// `swim-push-2` it broke the surface on tick 45 (air drag 0.91 from then).
#[test]
fn a_polar_bear_keeps_bds_horizontal_motion_in_water() {
    let fixture = fixture();
    for key in ["swim-push-0"] {
        let bds = &item(&fixture, key).actors[0];
        // The hit landed on tick 40; from tick 42 the bear coasts.
        let from = 42;
        let feet = bds.feet(from).unwrap() + DVec3::Y * DY;
        let mut a = pool(4, GameMode::Creative, false);
        let bear = summon(&mut a, POLAR_BEAR, "minecraft:ageable_grow_up", feet, 90.0);
        let world = a.world();
        world.entity_mut(bear).remove::<RandomStrollGoal>();
        world.get_mut::<Velocity>(bear).unwrap().0 = bds.velocity(from + 1);
        let mut previous = feet;
        for t in from + 1..from + 15 {
            a.ticks(1);
            let now = a.feet(bear);
            let (ours, theirs) = (now - previous, bds.velocity(t));
            assert!(
                (ours.x - theirs.x).abs() < 1e-4 && (ours.z - theirs.z).abs() < 1e-4,
                "{key} t {t}: moved {ours} vs BDS {theirs}"
            );
            previous = now;
        }
    }
}
