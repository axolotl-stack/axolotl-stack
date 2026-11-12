//! The slime and the magma cube replayed against BDS 1.26.51
//! (`mob/ai/testdata/bds-1.26.51-slime.json`, distilled by
//! `tools/bds-sweep/slime_fixture.py` from `sweep.py slime` and
//! `sweep.py slime-spawn`): their summons, hopping (the physics of a hop
//! exactly, the hop delays and the random turns by distribution over
//! seeded runs), splitting when they die and the slime chunks they spawn
//! in.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds;
use serde::Deserialize;
use unastar_data::actor_components::body::ActorDamageCause;

use super::facets::{Arena, floor};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, Health, OnGround, PhysicsBody, Position, Rotation};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::components::variant::appearance_id;
use crate::entity::mob::components::{HopMovementConfig, KnockbackPower};
use crate::entity::mob::constants::{SPLIT_MAX_CHILDREN, SPLIT_MIN_CHILDREN};
use crate::entity::mob::*;
use crate::entity::spawning::slime_chunk::is_slime_chunk;
use crate::world::rules::Difficulty;

const SLIME: &str = "minecraft:slime";
const MAGMA_CUBE: &str = "minecraft:magma_cube";
/// BDS's superflat floor is at y = -61, the arena's at 63.
const DY: f64 = 124.0;

#[derive(Debug, Deserialize)]
struct Fixture {
    items: Vec<Item>,
    #[serde(default)]
    spawns: Vec<Spawn>,
}

#[derive(Debug, Deserialize)]
struct Item {
    key: String,
    mob: Mob,
    size: Size,
    kind: Kind,
    trace: Vec<(u32, f64, f64, f64, u8, f32)>,
    #[serde(default)]
    hops: Vec<HopRow>,
    #[serde(default)]
    changes: Vec<(u32, u8, f32)>,
    #[serde(default)]
    split: Option<Split>,
    #[serde(default)]
    damage: Vec<Damage>,
}

#[derive(Debug, Deserialize)]
struct Damage {
    t: u32,
    amount: f32,
}

#[derive(Debug, Deserialize)]
struct HopRow {
    landed: Option<u32>,
    jumped: u32,
}

#[derive(Debug, Deserialize)]
struct Split {
    children: Vec<Child>,
}

#[derive(Debug, Deserialize)]
struct Child {
    pos: [f64; 3],
    variant: i32,
}

#[derive(Debug, Deserialize)]
struct Spawn {
    pos: [f64; 3],
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-slime.json"))
        .expect("slime fixture decodes")
}

/// The mob of a fixture item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mob {
    Slime,
    MagmaCube,
}

impl Mob {
    fn identifier(self) -> &'static str {
        match self {
            Mob::Slime => SLIME,
            Mob::MagmaCube => MAGMA_CUBE,
        }
    }
}

/// The size a fixture item summoned (`summon … spawn_<size>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    /// The size's `VARIANT`.
    fn variant(self) -> i32 {
        match self {
            Size::Small => 1,
            Size::Medium => 2,
            Size::Large => 4,
        }
    }
}

/// What a fixture item measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Hop,
    Attack,
    Split,
    Kill,
    Float,
}

fn mob_variant(world: &World, mob: Entity) -> i32 {
    appearance_id(
        world.get::<EntityMetadata>(mob).unwrap(),
        actor_data_i_ds::VARIANT,
    )
    .unwrap()
}

/// A mob of `identifier` whose spawn drew the size `variant`.
fn sized(a: &mut Arena, identifier: &str, variant: i32, feet: DVec3) -> Entity {
    for _ in 0..200 {
        let mob = a.adult(identifier, feet);
        if mob_variant(a.world(), mob) == variant {
            return mob;
        }
        a.world().despawn(mob);
    }
    panic!("no {identifier} of size {variant} in 200 spawns");
}

#[test]
fn spawned_slimes_add_actor_matches_native_summon() {
    for (identifier, bytes) in [
        (
            SLIME,
            &include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/slime.bin"
            )[..],
        ),
        (
            MAGMA_CUBE,
            &include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/magma_cube.bin"
            )[..],
        ),
    ] {
        let native = native(bytes);
        let size = appearance_id(
            &EntityMetadata::from_entries(native.actor_data.data.iter().cloned()),
            actor_data_i_ds::VARIANT,
        )
        .expect("the native summon has a size");
        let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
        let feet = DVec3::new(
            f64::from(native.position.x),
            f64::from(native.position.y),
            f64::from(native.position.z),
        );
        let mob = sized(&mut a, identifier, size, feet);
        assert_add_actor_matches(a.world(), identifier, mob, &native);
    }
}

/// The calm hop control of `identifier`.
fn calm_hop(identifier: &str) -> HopMovementConfig {
    let mut a = floor(1, Difficulty::Normal, GameMode::Creative);
    let mob = a.adult(identifier, DVec3::new(8.5, 64.0, 8.5));
    *a.world().get::<HopMovementConfig>(mob).unwrap()
}

/// One tick of a hopping mob: feet, ground contact and yaw.
type Sample = (DVec3, bool, f32);

fn sample(world: &World, mob: Entity) -> Sample {
    (
        world.get::<Position>(mob).unwrap().0,
        world.get::<OnGround>(mob).is_some_and(|g| g.0),
        world.get::<Rotation>(mob).unwrap().yaw,
    )
}

/// `ticks` ticks of a calm `identifier` of size `variant` alone in the
/// arena (the player is creative, so no target).
fn hop_run(seed: u64, identifier: &str, variant: i32, ticks: u32) -> Vec<Sample> {
    let mut a = floor(seed, Difficulty::Normal, GameMode::Creative);
    let mob = sized(&mut a, identifier, variant, DVec3::new(8.5, 64.0, 8.5));
    let mut out = vec![sample(a.world(), mob)];
    for _ in 0..ticks {
        a.ticks(1);
        out.push(sample(a.world(), mob));
    }
    out
}

/// Each hop's ground ticks (landing to leaving the ground) after the
/// first landing.
fn gaps(samples: impl Iterator<Item = bool>) -> Vec<u32> {
    let (mut out, mut ground, mut landed) = (Vec::new(), None::<bool>, None);
    for (t, on) in samples.enumerate() {
        let t = t as u32;
        if let Some(was) = ground {
            if on && !was {
                landed = Some(t);
            } else if was
                && !on
                && let Some(l) = landed
            {
                out.push(t - l);
            }
        }
        ground = Some(on);
    }
    out
}

/// Ground ticks between the random direction draws: counted from one turn
/// start to the next (a turn resumed after a hop interrupted it is not a
/// new draw, so intervals under ten ground ticks are dropped).
fn draw_intervals(samples: &[(bool, f32)]) -> Vec<u32> {
    let (mut out, mut ground_ticks, mut last, mut turning) = (Vec::new(), 0u32, None, false);
    for pair in samples.windows(2) {
        let ((_, before), (on, yaw)) = (pair[0], pair[1]);
        if on {
            ground_ticks += 1;
        }
        let delta = (yaw - before + 540.0).rem_euclid(360.0) - 180.0;
        if delta.abs() > 1e-3 && !turning {
            if let Some(l) = last
                && ground_ticks - l >= 10
            {
                out.push(ground_ticks - l);
            }
            last = Some(ground_ticks);
        }
        turning = delta.abs() > 1e-3;
    }
    out
}

fn mean(values: &[u32]) -> f64 {
    values.iter().map(|&v| f64::from(v)).sum::<f64>() / values.len() as f64
}

/// The standard error of the difference of two sample means.
fn mean_error(a: &[u32], b: &[u32]) -> f64 {
    let var = |v: &[u32]| {
        let m = mean(v);
        v.iter().map(|&x| (f64::from(x) - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64
    };
    (var(a) / a.len() as f64 + var(b) / b.len() as f64).sqrt()
}

/// The fixture's per-tick ground and yaw of an item, rebuilt from its
/// change list.
fn expand(item: &Item, ticks: u32) -> Vec<(bool, f32)> {
    let mut out = Vec::with_capacity(ticks as usize + 1);
    let mut changes = item.changes.iter().peekable();
    let mut current = (false, 0.0);
    for t in 0..=ticks {
        while let Some(&&(at, on, yaw)) = changes.peek() {
            if at > t {
                break;
            }
            current = (on != 0, yaw);
            changes.next();
        }
        out.push(current);
    }
    out
}

#[test]
fn hop_delays_and_random_turns_match_bds_by_distribution() {
    let fixture = fixture();
    // Per mob and size: BDS's hop delays and its turn intervals.
    type Measured = (Vec<u32>, Vec<u32>);
    let mut groups: BTreeMap<(Mob, Size), Measured> = BTreeMap::new();
    for item in fixture.items.iter().filter(|item| item.kind == Kind::Hop) {
        let entry = groups.entry((item.mob, item.size)).or_default();
        entry.0.extend(
            item.hops
                .iter()
                .filter_map(|hop| hop.landed.map(|l| hop.jumped - l)),
        );
        entry.1.extend(draw_intervals(&expand(item, 600)));
    }
    assert_eq!(groups.len(), 6, "every mob and size measured");
    let (mut bds_draws_all, mut our_draws_all) = (Vec::new(), Vec::new());
    for ((mob, size), (bds_gaps, bds_draws)) in &groups {
        let (mut ours, mut our_draws) = (Vec::new(), Vec::new());
        for seed in 0..6 {
            let run = hop_run(seed, mob.identifier(), size.variant(), 600);
            ours.extend(gaps(run.iter().map(|s| s.1)));
            let turns: Vec<(bool, f32)> = run.iter().map(|s| (s.1, s.2)).collect();
            our_draws.extend(draw_intervals(&turns));
        }
        // The delay is the authored seconds cut to whole ticks: both BDS's
        // and ours stay inside the cut range.
        let config = calm_hop(mob.identifier());
        let (lo, hi) = (config.delay_ticks(0.0), config.delay_ticks(0.99999));
        // unastar reports one more ground tick now and then (one hop in
        // about a hundred), when a landing settles over two ticks.
        for (who, gaps, hi) in [("BDS", bds_gaps, hi), ("ours", &ours, hi + 1)] {
            assert!(
                gaps.iter().all(|g| (lo..=hi).contains(g)),
                "{mob:?} {size:?}: {who} hop delays {gaps:?} outside {lo}..={hi}"
            );
        }
        let error = mean_error(bds_gaps, &ours);
        assert!(
            (mean(bds_gaps) - mean(&ours)).abs() < 3.0 * error,
            "{mob:?} {size:?}: mean hop delay {} vs BDS {}",
            mean(&ours),
            mean(bds_gaps)
        );
        bds_draws_all.extend(bds_draws);
        our_draws_all.extend(our_draws);
    }
    // A draw every 40 + 0..60 running ground ticks, the same for every mob
    // and size (pooled: about 16 draws per BDS item set). unastar turns a
    // landed mob one tick later than BDS (its AI runs before the landing
    // tick's physics), which can shorten one count by a tick.
    let draws = 39..=100;
    for (who, intervals) in [("BDS", &bds_draws_all), ("ours", &our_draws_all)] {
        assert!(
            intervals.iter().all(|d| draws.contains(d)),
            "{who} turn intervals {intervals:?}"
        );
    }
    assert!(
        (mean(&bds_draws_all) - mean(&our_draws_all)).abs()
            < 3.0 * mean_error(&bds_draws_all, &our_draws_all),
        "mean turn interval {} vs BDS {}",
        mean(&our_draws_all),
        mean(&bds_draws_all)
    );
}

#[test]
fn a_hop_moves_like_bds_tick_by_tick() {
    let fixture = fixture();
    let mut checked = 0;
    for item in fixture.items.iter().filter(|item| item.kind == Kind::Hop) {
        // The first hop from the ground in the fixture's trace.
        let Some(jump) = item
            .trace
            .windows(2)
            .position(|w| w[0].4 == 1 && w[1].4 == 0)
        else {
            continue;
        };
        let bds: Vec<(f64, f64)> = item.trace[jump..(jump + 14).min(item.trace.len())]
            .windows(2)
            .map(|w| {
                let horizontal = ((w[1].1 - w[0].1).powi(2) + (w[1].3 - w[0].3).powi(2)).sqrt();
                (w[1].2 - item.trace[jump].2, horizontal)
            })
            .collect();
        let run = hop_run(7, item.mob.identifier(), item.size.variant(), 200);
        let ours_jump = run
            .windows(2)
            .position(|w| w[0].1 && !w[1].1)
            .unwrap_or_else(|| panic!("{}: never hopped", item.key));
        let ours: Vec<(f64, f64)> = run[ours_jump..ours_jump + bds.len() + 1]
            .windows(2)
            .map(|w| {
                let d = w[1].0 - w[0].0;
                (
                    w[1].0.y - run[ours_jump].0.y,
                    (d.x * d.x + d.z * d.z).sqrt(),
                )
            })
            .collect();
        for (k, ((by, bh), (oy, oh))) in bds.iter().zip(&ours).enumerate() {
            assert!(
                (by - oy).abs() < 2e-3 && (bh - oh).abs() < 2e-3,
                "{} hop tick {k}: BDS rise {by:.4} step {bh:.4}, ours {oy:.4} {oh:.4}",
                item.key
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 18, "every hop item has a hop to compare");
}

/// Kill a slime of `identifier` and size `variant` standing at `feet` and
/// run its death animation; the children it split into.
fn split_run(seed: u64, identifier: &str, variant: i32, feet: DVec3) -> Vec<Entity> {
    let mut a = floor(seed, Difficulty::Normal, GameMode::Creative);
    let mob = sized(&mut a, identifier, variant, feet);
    let world = a.world();
    world
        .entity_mut(mob)
        .remove::<crate::entity::mob::ai::MobBrain>();
    combat::hurt_mob(
        world,
        mob,
        combat::Hit {
            damage: 100.0,
            cause: ActorDamageCause::EntityAttack,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    let children = children_after_death(&mut a, mob);
    for &child in &children {
        assert_eq!(mob_variant(a.world(), child), variant / 2);
    }
    children
}

/// The children of `mob`'s split, once its death animation ran.
fn children_after_death(a: &mut Arena, mob: Entity) -> Vec<Entity> {
    // Read on the tick the body goes, before the children move or push.
    for _ in 0..=constants::DEATH_TICKS + 2 {
        if a.world().get_entity(mob).is_err() {
            break;
        }
        a.ticks(1);
    }
    let world = a.world();
    world
        .query_filtered::<Entity, With<MobKind>>()
        .iter(world)
        .filter(|&child| child != mob)
        .collect()
}

#[test]
fn slimes_split_where_bds_puts_the_children() {
    let fixture = fixture();
    let mut checked = 0;
    // `/kill` deals no knockback: a body killed on the ground stays where
    // the plugin last saw the mob.
    for item in fixture.items.iter().filter(|item| item.kind == Kind::Kill) {
        let last = item.trace.last().unwrap();
        if last.4 == 0 {
            continue;
        }
        let feet = DVec3::new(last.1, last.2 + DY, last.3);
        let mut a = floor(3, Difficulty::Normal, GameMode::Creative);
        let mob = sized(&mut a, item.mob.identifier(), item.size.variant(), feet);
        a.world().get_mut::<Health>(mob).unwrap().current = 0.0;
        let children = children_after_death(&mut a, mob);
        let split = item.split.as_ref().unwrap();
        assert!(!children.is_empty(), "{}", item.key);
        for (child, bds) in children.iter().zip(&split.children) {
            let ours = a.world().get::<Position>(*child).unwrap().0;
            let bds_feet = DVec3::new(bds.pos[0], bds.pos[1] + DY, bds.pos[2]);
            assert!(
                (ours - bds_feet).length() < 1e-3,
                "{}: {ours} vs BDS {bds_feet}",
                item.key
            );
            assert_eq!(mob_variant(a.world(), *child), bds.variant, "{}", item.key);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn a_slime_splits_into_two_to_four_children_like_bds() {
    let fixture = fixture();
    let mut bds = BTreeMap::<usize, u32>::new();
    for item in fixture
        .items
        .iter()
        .filter(|item| matches!(item.kind, Kind::Split | Kind::Kill))
    {
        *bds.entry(item.split.as_ref().unwrap().children.len())
            .or_default() += 1;
    }
    let lo = SPLIT_MIN_CHILDREN as usize;
    let hi = SPLIT_MAX_CHILDREN as usize;
    assert!(
        bds.keys().all(|n| (lo..=hi).contains(n)),
        "BDS counts {bds:?}"
    );
    let mut ours = BTreeMap::<usize, u32>::new();
    for seed in 0..60 {
        let children = split_run(seed, SLIME, 2, DVec3::new(8.5, 64.0, 8.5));
        *ours.entry(children.len()).or_default() += 1;
    }
    assert_eq!(
        ours.keys().copied().collect::<Vec<_>>(),
        (lo..=hi).collect::<Vec<_>>()
    );
    assert!(ours.values().all(|&n| n >= 10), "roughly uniform: {ours:?}");
}

#[test]
fn split_children_stand_at_the_block_centre_and_take_half_the_size() {
    for (identifier, variant) in [(SLIME, 4), (SLIME, 2), (MAGMA_CUBE, 4), (MAGMA_CUBE, 2)] {
        let feet = DVec3::new(8.3, 64.0, 7.9);
        let mut a = floor(5, Difficulty::Normal, GameMode::Creative);
        let mob = sized(&mut a, identifier, variant, feet);
        let world = a.world();
        world
            .entity_mut(mob)
            .remove::<crate::entity::mob::ai::MobBrain>();
        world.get_mut::<Health>(mob).unwrap().current = 0.0;
        let children = children_after_death(&mut a, mob);
        let world = a.world();
        assert!(
            (SPLIT_MIN_CHILDREN as usize..=SPLIT_MAX_CHILDREN as usize).contains(&children.len()),
            "{identifier}"
        );
        for child in children {
            assert_eq!(mob_variant(world, child), variant / 2, "{identifier}");
            let body = world.get::<PhysicsBody>(child).unwrap();
            assert!(body.width < 2.0, "{identifier}: the size group applied");
        }
    }
    // The smallest size leaves nothing.
    let mut a = floor(5, Difficulty::Normal, GameMode::Creative);
    let mob = sized(&mut a, SLIME, 1, DVec3::new(8.5, 64.0, 8.5));
    a.world().get_mut::<Health>(mob).unwrap().current = 0.0;
    assert!(children_after_death(&mut a, mob).is_empty());
}

#[test]
fn every_slime_bds_spawned_stood_in_a_slime_chunk() {
    let fixture = fixture();
    assert!(!fixture.spawns.is_empty());
    for spawn in &fixture.spawns {
        let (cx, cz) = (
            (spawn.pos[0].floor() as i32).div_euclid(16),
            (spawn.pos[2].floor() as i32).div_euclid(16),
        );
        assert!(
            is_slime_chunk(cx, cz),
            "a slime spawned in chunk {cx}, {cz}"
        );
    }
}

/// The hits a survival player standing at the arena's west edge takes from
/// a `variant`-sized `identifier` starting 5 blocks east, over `ticks`:
/// `(tick, damage)`.
fn hits(seed: u64, identifier: &str, variant: i32, ticks: u32) -> Vec<(u32, f32)> {
    let mut a = floor(seed, Difficulty::Normal, GameMode::Survival);
    a.move_player(DVec3::new(0.5, 64.0, 0.5));
    let player = a.player;
    sized(&mut a, identifier, variant, DVec3::new(5.5, 64.0, 0.5));
    let mut out = Vec::new();
    for t in 0..ticks {
        *a.world().get_mut::<Health>(player).unwrap() = Health {
            current: 1000.0,
            max: 1000.0,
        };
        a.ticks(1);
        let health = a.world().get::<Health>(player).unwrap().current;
        if health < 1000.0 {
            out.push((t, 1000.0 - health));
        }
    }
    out
}

#[test]
fn slimes_hurt_a_touching_player_like_bds() {
    let fixture = fixture();
    let mut checked = 0;
    for item in fixture
        .items
        .iter()
        .filter(|item| item.kind == Kind::Attack)
    {
        let ours = hits(9, item.mob.identifier(), item.size.variant(), 400);
        if item.damage.is_empty() {
            // The small slime has no area attack and attack 0.
            assert!(ours.is_empty(), "{}: {ours:?}", item.key);
            continue;
        }
        let bds_amounts: Vec<f32> = item.damage.iter().map(|d| d.amount).collect();
        assert!(!ours.is_empty(), "{}: never hurt the player", item.key);
        for (_, amount) in &ours {
            assert!(
                bds_amounts.contains(amount),
                "{}: {amount} vs BDS {bds_amounts:?}",
                item.key
            );
        }
        // Never faster than the player's hurt immunity, as on BDS.
        let min_gap = |ticks: Vec<u32>| ticks.windows(2).map(|w| w[1] - w[0]).min();
        let bds_gap = min_gap(item.damage.iter().map(|d| d.t).collect());
        let our_gap = min_gap(ours.iter().map(|h| h.0).collect());
        if let (Some(bds), Some(our)) = (bds_gap, our_gap) {
            assert!(
                bds >= 10 && our >= 10,
                "{}: BDS {bds}, ours {our}",
                item.key
            );
        }
        checked += 1;
    }
    assert!(checked > 0);
}
