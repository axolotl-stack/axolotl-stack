//! The silverfish replayed against BDS 1.26.51
//! (`mob/ai/testdata/bds-1.26.51-silverfish.json`, distilled by
//! `tools/bds-sweep/silverfish_fixture.py` from `sweep.py silverfish`):
//! its summon, merging into the blocks of a pit (which blocks, what they
//! become, and when, compared by distribution over seeded runs) and waking
//! the silverfish hidden in infested stone around it.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;
use serde::Deserialize;

use super::facets::{Arena, arena, block};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{ActiveEffect, EffectType, GameMode, Position};
use crate::entity::mob::ai::silverfish::scan_order;
use crate::entity::mob::components::InfestedBlocks;
use crate::entity::mob::constants::{
    SILVERFISH_RELEASE_OFFSET_X, SILVERFISH_WAKE_RANGE_XZ, SILVERFISH_WAKE_RANGE_Y,
};
use crate::entity::mob::state::AttackTarget;
use crate::entity::mob::*;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::server::game::types::block_network_ids;
use crate::world::rules::{Difficulty, GameRules};

const SILVERFISH: &str = "minecraft:silverfish";
/// BDS's superflat floor is at y = -61, the arena's at 63.
const DY: i32 = 124;

#[derive(Debug, Deserialize)]
struct Fixture {
    merge: Vec<MergeItem>,
    wake: Vec<WakeItem>,
}

#[derive(Debug, Deserialize)]
struct MergeItem {
    key: String,
    block: String,
    states: BTreeMap<String, String>,
    griefing: bool,
    feet: [i32; 3],
    ticks: u32,
    merge_anim: Option<u32>,
    changed: Option<Changed>,
}

#[derive(Debug, Deserialize)]
struct Changed {
    pos: [i32; 3],
    network_id: u32,
}

#[derive(Debug, Deserialize)]
struct WakeItem {
    key: String,
    griefing: bool,
    feet: [i32; 3],
    hurt: u32,
    ticks: u32,
    cells: Vec<[i32; 3]>,
    events: Vec<WakeEvent>,
}

#[derive(Debug, Deserialize)]
struct WakeEvent {
    t: u32,
    destroyed: Vec<BlockAt>,
    set: Vec<BlockAt>,
    released: Vec<[f64; 3]>,
}

#[derive(Debug, Deserialize)]
struct BlockAt {
    pos: [i32; 3],
    network_id: i64,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../ai/testdata/bds-1.26.51-silverfish.json"))
        .expect("silverfish fixture decodes")
}

fn ivec(v: [i32; 3]) -> IVec3 {
    IVec3::from_array(v)
}

/// `name` with the fixture's string states set.
fn state(name: &str, states: &BTreeMap<String, String>) -> u32 {
    use crate::registry::block::StateValue;
    let entry = BlockRegistry::vanilla().get_by_name(name).expect(name);
    states
        .iter()
        .fold(entry.default_state_id, |out, (property, value)| {
            let value = entry
                .data
                .properties
                .iter()
                .filter(|p| p.name == property)
                .flat_map(|p| p.values.iter().copied())
                .find(|v| matches!(v, StateValue::Str(s) if s == value))
                .unwrap_or_else(|| panic!("{name} has no {property}={value}"));
            entry
                .with_state_value(out, property, value)
                .unwrap_or_else(|| panic!("{name} has no {property}={value:?}"))
        })
}

#[test]
fn spawned_silverfish_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/silverfish.bin"
    ));
    let mut a = super::facets::floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob = a.adult(SILVERFISH, feet);
    assert_add_actor_matches(a.world(), SILVERFISH, mob, &native);
}

#[test]
fn the_measured_infested_blocks_are_what_bds_merged_into() {
    let blocks = InfestedBlocks::vanilla().unwrap();
    let fixture = fixture();
    let mut a = super::facets::floor(1, Difficulty::Normal, GameMode::Creative);
    let wire = block_network_ids(a.world());
    let mut checked = 0;
    for item in fixture.merge.iter().filter(|item| item.griefing) {
        let host = state(&item.block, &item.states);
        let ours = blocks.infested_state(host);
        match &item.changed {
            Some(changed) => {
                let infested = ours.unwrap_or_else(|| panic!("{}: not a host", item.key));
                assert_eq!(wire.to_wire(infested), changed.network_id, "{}", item.key);
                assert_eq!(blocks.host_state(infested), Some(host), "{}", item.key);
                checked += 1;
            }
            None => assert_eq!(ours, None, "{}: BDS never merged", item.key),
        }
    }
    assert!(checked > 0);
}

/// A 3 × 3 × 4 block of `host` with a 1 × 1 pit two deep at (5, 64, 1),
/// as the BDS sweep cut it.
fn pit(seed: u64, host: u32, griefing: bool) -> (Arena, Entity) {
    let mut a = arena(seed, Difficulty::Normal, GameMode::Creative, move |pos| {
        let inside =
            (4..=6).contains(&pos.x) && (63..=66).contains(&pos.y) && (0..=2).contains(&pos.z);
        let hole = pos.x == 5 && pos.z == 1 && (64..=65).contains(&pos.y);
        (inside && !hole).then_some(host)
    });
    a.world().resource_mut::<GameRules>().mob_griefing = griefing;
    let mob = a.adult(SILVERFISH, DVec3::new(5.5, 64.0, 1.5));
    (a, mob)
}

/// Run a pit until the silverfish merges: the tick it went and the block
/// that turned infested (relative to its feet block).
fn merge_run(seed: u64, host: u32, ticks: u32) -> Option<(u32, IVec3, u32)> {
    let (mut a, mob) = pit(seed, host, true);
    let feet = IVec3::new(5, 64, 1);
    for t in 0..ticks {
        a.ticks(1);
        if a.world().get_entity(mob).is_err() {
            let world = a.world();
            for face in [
                IVec3::NEG_Y,
                IVec3::Y,
                IVec3::NEG_Z,
                IVec3::Z,
                IVec3::NEG_X,
                IVec3::X,
            ] {
                let state = world.block_at(feet + face).unwrap();
                if state != host && state != *crate::world::chunk::blocks::AIR {
                    return Some((t, face, block_network_ids(world).to_wire(state)));
                }
            }
            panic!("seed {seed}: the silverfish left without changing a block");
        }
    }
    None
}

#[test]
fn a_silverfish_merges_into_its_pit_as_often_as_on_bds() {
    let fixture = fixture();
    // The 1 × 1 pits of every host block measured on a quiet machine (the
    // `pit` sweeps ran beside a compile and waited about twice as long; see
    // docs/research/silverfish-bds-1.26.51.md).
    let pits: Vec<&MergeItem> = fixture
        .merge
        .iter()
        .filter(|item| {
            item.griefing
                && (item.key.starts_with("ab-") || item.key.starts_with("blocks-"))
                && item.changed.is_some()
        })
        .collect();
    assert!(pits.len() >= 100, "{} pits", pits.len());
    let bds: Vec<u32> = pits
        .iter()
        .map(|item| {
            item.merge_anim
                .expect("a changed block comes with the merge event")
        })
        .collect();
    // BDS merged into every face of the feet block but the open one above.
    let bds_faces: std::collections::BTreeSet<[i32; 3]> = pits
        .iter()
        .map(|item| {
            let changed = item.changed.as_ref().expect("a merge changes a block");
            (ivec(changed.pos) - ivec(item.feet)).to_array()
        })
        .collect();
    assert_eq!(bds_faces.len(), 5, "{bds_faces:?}");
    assert!(!bds_faces.contains(&[0, 1, 0]));
    let stone = block("minecraft:stone");
    let infested =
        block_network_ids(super::facets::floor(1, Difficulty::Normal, GameMode::Creative).world())
            .to_wire(block("minecraft:infested_stone"));
    let runs = 400;
    let mut ours = Vec::new();
    let mut faces = BTreeMap::new();
    for seed in 0..runs {
        let (t, face, wire) = merge_run(seed, stone, 400).expect("merged within 400 ticks");
        assert_eq!(wire, infested);
        assert_ne!(face, IVec3::Y, "never into the open block above");
        *faces.entry(face.to_array()).or_insert(0) += 1;
        ours.push(t);
    }
    assert_eq!(faces.len(), 5, "{faces:?}");
    // Merges are a per-tick hazard: compare the mean wait (geometric, so its
    // standard error is about the mean over √n) against BDS's.
    let mean = |v: &[u32]| v.iter().map(|&t| f64::from(t)).sum::<f64>() / v.len() as f64;
    let (bds_mean, our_mean) = (mean(&bds), mean(&ours));
    let error = (bds_mean / (bds.len() as f64).sqrt()).hypot(our_mean / (ours.len() as f64).sqrt());
    assert!(
        (bds_mean - our_mean).abs() < 3.0 * error,
        "BDS waits {bds_mean:.1} ticks ({} runs), we wait {our_mean:.1}",
        bds.len()
    );
}

#[test]
fn without_mob_griefing_a_silverfish_never_merges() {
    let fixture = fixture();
    let bds: Vec<&MergeItem> = fixture.merge.iter().filter(|item| !item.griefing).collect();
    assert!(!bds.is_empty() && bds.iter().all(|item| item.merge_anim.is_none()));
    let stone = block("minecraft:stone");
    for seed in 0..5 {
        let (mut a, mob) = pit(seed, stone, false);
        a.ticks(u64::from(bds[0].ticks));
        assert!(a.world().get_entity(mob).is_ok(), "seed {seed} merged");
    }
}

/// The wake-up scan's order over the fixture's cells, relative to `feet`.
fn scan_rank(feet: IVec3) -> impl Fn(IVec3) -> usize {
    let order: Vec<IVec3> = scan_order(SILVERFISH_WAKE_RANGE_Y)
        .flat_map(|dy| {
            scan_order(SILVERFISH_WAKE_RANGE_XZ).flat_map(move |dx| {
                scan_order(SILVERFISH_WAKE_RANGE_XZ).map(move |dz| IVec3::new(dx, dy, dz))
            })
        })
        .collect();
    move |pos| {
        order
            .iter()
            .position(|offset| feet + *offset == pos)
            .unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Default)]
struct WakeRun {
    /// Per scan tick (relative to the hurt), the blocks broken in order.
    rounds: Vec<(u32, Vec<IVec3>)>,
    /// Released silverfish feet.
    released: Vec<DVec3>,
}

fn bds_wake(item: &WakeItem) -> WakeRun {
    let mut run = WakeRun::default();
    for event in &item.events {
        if !event.destroyed.is_empty() {
            run.rounds.push((
                event.t - item.hurt,
                event.destroyed.iter().map(|d| ivec(d.pos)).collect(),
            ));
        }
        run.released
            .extend(event.released.iter().map(|&p| DVec3::from_array(p)));
    }
    run
}

fn our_wake(seed: u64, item: &WakeItem) -> WakeRun {
    let cells: Vec<IVec3> = item
        .cells
        .iter()
        .map(|&c| ivec(c) + IVec3::Y * DY)
        .collect();
    let infested = block("minecraft:infested_stone");
    let lifted = cells.clone();
    let mut a = arena(seed, Difficulty::Normal, GameMode::Survival, move |pos| {
        lifted.contains(&pos).then_some(infested)
    });
    a.world().resource_mut::<GameRules>().mob_griefing = item.griefing;
    let feet = ivec(item.feet) + IVec3::Y * DY;
    let mob = a.adult(SILVERFISH, feet.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
    // Frozen like the BDS silverfish (hidden Slowness 255), and the player
    // stands 4 blocks west.
    crate::entity::effects::give(
        a.world(),
        mob,
        EffectType::SLOWNESS,
        ActiveEffect::new(255, 100_000),
    );
    let player = a.player;
    a.world().get_mut::<Position>(player).unwrap().0 =
        feet.as_dvec3() + DVec3::new(-3.5, crate::entity::PLAYER_EYE_HEIGHT, 0.5);
    let mut run = WakeRun::default();
    // The BDS silverfish took the player as its target (and turned angry)
    // on its first ticks; this one is given it, and the timeline starts once
    // its angry group is on.
    crate::entity::mob::ai::provoke(a.world(), mob, player);
    a.ticks(2);
    assert_eq!(
        a.world().get::<AttackTarget>(mob),
        Some(&AttackTarget(player))
    );
    let mut present: Vec<IVec3> = cells.clone();
    a.packets();
    for t in 0..=item.ticks {
        if t == item.hurt {
            let world = a.world();
            combat::player_attack(world, player, mob, &Default::default());
        }
        a.ticks(1);
        let world = a.world();
        let gone: Vec<IVec3> = present
            .iter()
            .copied()
            .filter(|&pos| world.block_at(pos) != Some(infested))
            .collect();
        present.retain(|pos| !gone.contains(pos));
        if !gone.is_empty() {
            // Blocks broken on one tick, in the scan's order.
            let rank = scan_rank(feet);
            let mut round = gone;
            round.sort_by_key(|&pos| rank(pos));
            run.rounds.push((
                t - item.hurt,
                round.iter().map(|p| *p - IVec3::Y * DY).collect(),
            ));
        }
        // Where viewers first see each released silverfish.
        for packet in a.packets() {
            if let McpePacketData::AddActorPacket(add) = packet
                && &*add.actor_type == SILVERFISH
            {
                let at = DVec3::new(
                    f64::from(add.position.x),
                    f64::from(add.position.y),
                    f64::from(add.position.z),
                );
                if t > 0 {
                    run.released.push(at - DVec3::Y * f64::from(DY));
                }
            }
        }
    }
    run
}

#[test]
fn a_hurt_silverfish_wakes_the_infested_stone_around_it_like_bds() {
    let fixture = fixture();
    let griefing: Vec<&WakeItem> = fixture.wake.iter().filter(|item| item.griefing).collect();
    assert!(!griefing.is_empty());
    let mut bds_sizes = Vec::new();
    let mut our_sizes = Vec::new();
    for (seed, item) in griefing.iter().cycle().take(12).enumerate() {
        let rank = scan_rank(ivec(item.feet));
        let bds = bds_wake(item);
        let ours = our_wake(seed as u64, item);
        for (name, run) in [("BDS", &bds), ("ours", &ours)] {
            // Every 20 ticks from 20 ticks after the hurt.
            for (t, _) in &run.rounds {
                assert!(
                    *t >= 20 && t % 20 == 0,
                    "{name} {}: a scan at +{t}",
                    item.key
                );
            }
            // Each scan breaks the next blocks in scan order (BDS's destroy
            // particles come in the order it broke them).
            let order: Vec<IVec3> = run
                .rounds
                .iter()
                .flat_map(|(_, b)| b.iter().copied())
                .collect();
            let ranks: Vec<usize> = order.iter().map(|&p| rank(p)).collect();
            assert!(
                ranks.windows(2).all(|w| w[0] < w[1]),
                "{name} {}: {:?}",
                item.key,
                run.rounds
            );
            // A released silverfish stands at its block's corner, moved along x.
            for feet in &run.released {
                let corner = DVec3::new(feet.x - SILVERFISH_RELEASE_OFFSET_X, feet.y, feet.z);
                assert!(
                    (corner - corner.round()).abs().max_element() < 1e-4,
                    "{name} {}: released at {feet}",
                    item.key
                );
            }
        }
        assert_eq!(ours.rounds.first().map(|r| r.0), Some(20), "{}", item.key);
        assert_eq!(
            ours.released.len(),
            ours.rounds.iter().map(|r| r.1.len()).sum::<usize>()
        );
        if seed < griefing.len() {
            bds_sizes.extend(bds.rounds.iter().map(|r| r.1.len()));
        }
        our_sizes.extend(ours.rounds.iter().map(|r| r.1.len()));
    }
    // A scan stops after each block on a fair coin: half the scans break one
    // block, on BDS and here.
    let ones = |v: &[usize]| v.iter().filter(|&&n| n == 1).count() as f64 / v.len() as f64;
    assert!((ones(&bds_sizes) - 0.5).abs() < 0.25, "BDS {bds_sizes:?}");
    assert!((ones(&our_sizes) - 0.5).abs() < 0.15, "ours {our_sizes:?}");
}

#[test]
fn without_mob_griefing_woken_blocks_turn_back_into_stone() {
    let fixture = fixture();
    let item = fixture
        .wake
        .iter()
        .find(|item| !item.griefing)
        .expect("the fixture has a wake-nogrief run");
    let mut a = super::facets::floor(1, Difficulty::Normal, GameMode::Creative);
    let stone = block_network_ids(a.world()).to_wire(block("minecraft:stone"));
    // BDS: no particles and no silverfish, the woken blocks set to stone.
    let bds_set: Vec<&BlockAt> = item
        .events
        .iter()
        .filter(|e| e.t < item.ticks)
        .flat_map(|e| e.set.iter())
        .collect();
    assert!(
        item.events
            .iter()
            .all(|e| e.destroyed.is_empty() && e.released.is_empty())
    );
    assert!(!bds_set.is_empty());
    assert!(bds_set.iter().all(|b| b.network_id == i64::from(stone)));
    let ours = our_wake(3, item);
    assert!(ours.released.is_empty());
    assert_eq!(ours.rounds.first().map(|r| r.0), Some(20));
    let woken: Vec<IVec3> = ours
        .rounds
        .iter()
        .flat_map(|r| r.1.iter().copied())
        .collect();
    assert!(!woken.is_empty());
}
