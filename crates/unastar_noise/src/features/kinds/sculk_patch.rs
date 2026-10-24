//! `minecraft:sculk_patch`: a world-generation run of the sculk spreader.
//!
//! Vanilla grows sculk patches by simulating `SculkSpreader` in its
//! world-generation mode (`SculkSpreader.createWorldGenSpreader()`): growth
//! spawn cost 50, no-growth radius 1, charge decay rate 5, additional decay
//! rate 10, replacing `#sculk_replaceable_world_gen`. Charge cursors start at
//! the origin, hop between sculk blocks and veins, convert replaceable blocks
//! touched by veins into sculk (spread rounds only), grow veins around new
//! sculk, and spend charge on sculk sensors / shriekers.
//!
//! The simulation reproduces vanilla's cursor list, delays, block behaviours
//! (`SculkBlock`, `SculkVeinBlock` and the default behaviour for every other
//! block) and their random draws in order. Behaviour that has no effect on
//! world generation is left out: sounds, level events, entity pushing, the
//! cursor merge map (world-generation spreaders never merge cursors), and
//! the far-away cursor guard (a cursor dies once it leaves a 15 block
//! horizontal radius, long before that guard could trigger). Vein
//! attachment uses the sturdy-face approximation documented in
//! [`super::multiface_growth`].

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::SculkPatchConfig;
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::kinds::multiface_growth::{
    Spreader, all_directions_shuffled, can_attach_to, has_face, java_shuffle,
};
use crate::features::random::FeatureRandom;

/// World-generation spreader parameters.
const GROWTH_SPAWN_COST: i32 = 50;
const NO_GROWTH_RADIUS: i32 = 1;
const CHARGE_DECAY_RATE: i32 = 5;
const ADDITIONAL_DECAY_RATE: i32 = 10;
const MAX_CURSORS: usize = 32;
const MAX_CHARGE: i32 = 1000;

/// `ChargeCursor.NON_CORNER_NEIGHBOURS`: the 3x3x3 cube around a block
/// (x fastest, then y, then z) without the centre and the eight corners.
const NON_CORNER_NEIGHBOURS: [[i32; 3]; 18] = {
    let mut out = [[0; 3]; 18];
    let mut n = 0;
    let mut z = -1;
    while z <= 1 {
        let mut y = -1;
        while y <= 1 {
            let mut x = -1;
            while x <= 1 {
                if (x == 0 || y == 0 || z == 0) && !(x == 0 && y == 0 && z == 0) {
                    out[n] = [x, y, z];
                    n += 1;
                }
                x += 1;
            }
            y += 1;
        }
        z += 1;
    }
    out
};

/// `SculkBehaviour` of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    Sculk,
    Vein,
    Default,
}

impl Behaviour {
    #[inline]
    fn of(state: JavaState) -> Behaviour {
        if state.is(b::SCULK) {
            Behaviour::Sculk
        } else if state.is(b::SCULK_VEIN) {
            Behaviour::Vein
        } else {
            Behaviour::Default
        }
    }
}

/// `SculkSpreader.ChargeCursor`.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    pos: BlockPos,
    charge: i32,
    update_delay: i32,
    decay_delay: i32,
    /// `facings`: `None` until the cursor first rests on a sculk block or
    /// vein, then that block's faces as a bitmask over `Direction as u8`.
    facings: Option<u8>,
}

#[inline]
fn is_water_source(state: JavaState) -> bool {
    let fluid = state.fluid();
    fluid.is_water() && fluid.source
}

/// `MultifaceBlock.availableFaces`.
fn available_faces(state: JavaState) -> u8 {
    if !state.is(b::SCULK_VEIN) {
        return 0;
    }
    let mut mask = 0;
    for dir in Direction::ALL {
        if has_face(state, dir) {
            mask |= 1 << dir as u8;
        }
    }
    mask
}

/// `SculkVeinBlock.regrow`: a vein with every listed face that can attach,
/// waterlogged over any fluid.
fn regrow<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    state: JavaState,
    faces: u8,
) -> bool {
    let mut vein = states::SCULK_VEIN;
    let mut any = false;
    for dir in Direction::ALL {
        if faces & (1 << dir as u8) != 0 && can_attach_to(world.get_state(pos.rel(dir)), dir) {
            vein = vein.with_bool(dir.name(), true);
            any = true;
        }
    }
    if !any {
        return false;
    }
    if !state.fluid().is_empty() {
        vein = vein.with_bool("waterlogged", true);
    }
    world.set_state(pos, vein);
    true
}

/// `SculkBehaviour.attemptSpreadVein`.
fn attempt_spread_vein<W: FeatureWorld + ?Sized>(
    world: &mut W,
    behaviour: Behaviour,
    pos: BlockPos,
    state: JavaState,
    facings: Option<u8>,
) -> bool {
    match (behaviour, facings) {
        (Behaviour::Default, None) => {
            let current = world.get_state(pos);
            Spreader::SculkVeinSameSpace.spread_all(world, current, pos) > 0
        }
        (Behaviour::Default, Some(faces)) if faces != 0 => {
            if !state.is_air() && !is_water_source(state) {
                false
            } else {
                regrow(world, pos, state, faces)
            }
        }
        _ => Spreader::SculkVein.spread_all(world, state, pos) > 0,
    }
}

/// `SculkVeinBlock.onDischarged`: drops faces resting on sculk, and turns a
/// faceless vein into air or water.
fn vein_on_discharged<W: FeatureWorld + ?Sized>(world: &mut W, state: JavaState, pos: BlockPos) {
    if !state.is(b::SCULK_VEIN) {
        return;
    }
    let mut s = state;
    for dir in Direction::ALL {
        if has_face(s, dir) && world.get_state(pos.rel(dir)).is(b::SCULK) {
            s = s.with_bool(dir.name(), false);
        }
    }
    if !Direction::ALL.iter().any(|d| has_face(s, *d)) {
        s = if world.get_state(pos).fluid().is_empty() {
            states::AIR
        } else {
            states::WATER
        };
    }
    world.set_state(pos, s);
}

#[inline]
fn on_discharged<W: FeatureWorld + ?Sized>(
    world: &mut W,
    behaviour: Behaviour,
    state: JavaState,
    pos: BlockPos,
) {
    if behaviour == Behaviour::Vein {
        vein_on_discharged(world, state, pos);
    }
}

/// `SculkBlock.canPlaceGrowth`: open (air or water source) above and at
/// most two sensors/shriekers in the 9x3x9 box above the block.
fn can_place_growth<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let above = world.get_state(pos.above());
    if !(above.is_air() || (above.is(b::WATER) && is_water_source(above))) {
        return false;
    }
    let mut count = 0;
    for dz in -4..=4 {
        for dy in 0..=2 {
            for dx in -4..=4 {
                let s = world.get_state(pos.offset(dx, dy, dz));
                if s.is(b::SCULK_SENSOR) || s.is(b::SCULK_SHRIEKER) {
                    count += 1;
                }
                if count > 2 {
                    return false;
                }
            }
        }
    }
    true
}

/// `SculkBlock.getRandomGrowthState`: a shrieker (able to summon, as in
/// world generation) with 1/11, else a sensor; waterlogged over fluid.
fn random_growth_state<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    rng: &mut FeatureRandom,
) -> JavaState {
    let state = if rng.next_int_bounded(11) == 0 {
        states::SCULK_SHRIEKER.with_bool("can_summon", true)
    } else {
        states::SCULK_SENSOR
    };
    if state.has("waterlogged") && !world.get_state(pos).fluid().is_empty() {
        state.with_bool("waterlogged", true)
    } else {
        state
    }
}

/// `SculkBlock.getDecayPenalty` (float arithmetic as in Java).
fn decay_penalty(pos: BlockPos, origin: BlockPos, charge: i32) -> i32 {
    let dist = pos.dist_sqr(origin).sqrt() as f32 - NO_GROWTH_RADIUS as f32;
    let d = dist * dist;
    let r = 24 - NO_GROWTH_RADIUS;
    let f = (d / (r * r) as f32).min(1.0);
    1.max((charge as f32 * f * 0.5) as i32)
}

/// `SculkVeinBlock.attemptPlaceSculk`: converts the first replaceable block
/// behind one of the vein's faces (shuffled order) into sculk, spreads veins
/// around it and discharges neighbouring veins.
fn attempt_place_sculk<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    rng: &mut FeatureRandom,
) -> bool {
    let state = world.get_state(pos);
    for dir in all_directions_shuffled(rng) {
        if !has_face(state, dir) {
            continue;
        }
        let target = pos.rel(dir);
        if !world
            .get_state(target)
            .is_in(tags::SCULK_REPLACEABLE_WORLD_GEN)
        {
            continue;
        }
        world.set_state(target, states::SCULK);
        Spreader::SculkVein.spread_all(world, states::SCULK, target);
        let back = dir.opposite();
        for d2 in Direction::ALL {
            if d2 == back {
                continue;
            }
            let p2 = target.rel(d2);
            let s2 = world.get_state(p2);
            if s2.is(b::SCULK_VEIN) {
                vein_on_discharged(world, s2, p2);
            }
        }
        return true;
    }
    false
}

/// `SculkBehaviour.attemptUseCharge`: the cursor's remaining charge.
fn attempt_use_charge<W: FeatureWorld + ?Sized>(
    world: &mut W,
    behaviour: Behaviour,
    cursor: &Cursor,
    origin: BlockPos,
    rng: &mut FeatureRandom,
    convert: bool,
) -> i32 {
    let charge = cursor.charge;
    match behaviour {
        Behaviour::Default => {
            if cursor.decay_delay > 0 {
                charge
            } else {
                0
            }
        }
        Behaviour::Vein => {
            if convert && attempt_place_sculk(world, cursor.pos, rng) {
                charge - 1
            } else if rng.next_int_bounded(CHARGE_DECAY_RATE) == 0 {
                (charge as f32 * 0.5).floor() as i32
            } else {
                charge
            }
        }
        Behaviour::Sculk => {
            if charge == 0 || rng.next_int_bounded(CHARGE_DECAY_RATE) != 0 {
                return charge;
            }
            let pos = cursor.pos;
            let radius = NO_GROWTH_RADIUS as f64;
            let near = pos.dist_sqr(origin) < radius * radius;
            if !near && can_place_growth(world, pos) {
                if rng.next_int_bounded(GROWTH_SPAWN_COST) < charge {
                    let above = pos.above();
                    let growth = random_growth_state(world, above, rng);
                    world.set_state(above, growth);
                }
                0.max(charge - GROWTH_SPAWN_COST)
            } else if rng.next_int_bounded(ADDITIONAL_DECAY_RATE) != 0 {
                charge
            } else {
                charge
                    - if near {
                        1
                    } else {
                        decay_penalty(pos, origin, charge)
                    }
            }
        }
    }
}

/// `ChargeCursor.isUnobstructed`.
#[inline]
fn is_unobstructed<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos, dir: Direction) -> bool {
    !world.get_state(pos.rel(dir)).is_face_sturdy(dir.opposite())
}

/// `ChargeCursor.isMovementUnobstructed`: diagonal moves need one of the two
/// intermediate faces to be open.
fn is_movement_unobstructed<W: FeatureWorld + ?Sized>(
    world: &mut W,
    from: BlockPos,
    to: BlockPos,
) -> bool {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    if dx.abs() + dy.abs() + dz.abs() == 1 {
        return true;
    }
    let x_dir = if dx < 0 {
        Direction::West
    } else {
        Direction::East
    };
    let y_dir = if dy < 0 {
        Direction::Down
    } else {
        Direction::Up
    };
    let z_dir = if dz < 0 {
        Direction::North
    } else {
        Direction::South
    };
    if dx == 0 {
        is_unobstructed(world, from, y_dir) || is_unobstructed(world, from, z_dir)
    } else if dy == 0 {
        is_unobstructed(world, from, x_dir) || is_unobstructed(world, from, z_dir)
    } else {
        is_unobstructed(world, from, x_dir) || is_unobstructed(world, from, y_dir)
    }
}

/// `SculkVeinBlock.hasSubstrateAccess`: a vein face rests on
/// `#sculk_replaceable`.
fn has_substrate_access<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
) -> bool {
    if !state.is(b::SCULK_VEIN) {
        return false;
    }
    for dir in Direction::ALL {
        if has_face(state, dir) && world.get_state(pos.rel(dir)).is_in(tags::SCULK_REPLACEABLE) {
            return true;
        }
    }
    false
}

/// `ChargeCursor.getValidMovementPos`: the last reachable sculk block or
/// vein among the shuffled non-corner neighbours, stopping early at a vein
/// with substrate access.
fn valid_movement_pos<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    rng: &mut FeatureRandom,
) -> Option<BlockPos> {
    let mut offsets = NON_CORNER_NEIGHBOURS;
    java_shuffle(&mut offsets, rng);
    let mut result = pos;
    for [x, y, z] in offsets {
        let n = pos.offset(x, y, z);
        let s = world.get_state(n);
        if Behaviour::of(s) != Behaviour::Default && is_movement_unobstructed(world, pos, n) {
            result = n;
            if has_substrate_access(world, s, n) {
                break;
            }
        }
    }
    (result != pos).then_some(result)
}

/// `ChargeCursor.update` in world-generation mode.
fn update_cursor<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cursor: &mut Cursor,
    origin: BlockPos,
    rng: &mut FeatureRandom,
    convert: bool,
) {
    if cursor.charge <= 0 {
        return;
    }
    if cursor.update_delay > 0 {
        cursor.update_delay -= 1;
        return;
    }
    let mut state = world.get_state(cursor.pos);
    let mut behaviour = Behaviour::of(state);
    if convert && attempt_spread_vein(world, behaviour, cursor.pos, state, cursor.facings) {
        // Every behaviour except sculk's may change the block on spread.
        if behaviour != Behaviour::Sculk {
            state = world.get_state(cursor.pos);
            behaviour = Behaviour::of(state);
        }
    }
    cursor.charge = attempt_use_charge(world, behaviour, cursor, origin, rng, convert);
    if cursor.charge <= 0 {
        on_discharged(world, behaviour, state, cursor.pos);
        return;
    }
    if let Some(next) = valid_movement_pos(world, cursor.pos, rng) {
        on_discharged(world, behaviour, state, cursor.pos);
        cursor.pos = next;
        let centre = BlockPos::new(origin.x, next.y, origin.z);
        if next.dist_sqr(centre) >= 15.0 * 15.0 {
            cursor.charge = 0;
            return;
        }
        state = world.get_state(next);
    }
    if Behaviour::of(state) != Behaviour::Default {
        cursor.facings = Some(available_faces(state));
    }
    cursor.decay_delay = match behaviour {
        Behaviour::Default => (cursor.decay_delay - 1).max(0),
        Behaviour::Sculk | Behaviour::Vein => 1,
    };
    cursor.update_delay = 1;
}

/// `SculkPatchFeature.canSpreadFrom`.
fn can_spread_from<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let state = world.get_state(pos);
    if Behaviour::of(state) != Behaviour::Default {
        return true;
    }
    if !state.is_air() && !(state.is(b::WATER) && state.fluid().source) {
        return false;
    }
    Direction::ALL
        .into_iter()
        .any(|dir| world.get_state(pos.rel(dir)).is_collision_full_cube())
}

/// Java `minecraft:sculk_patch` (`SculkPatchFeature`).
///
/// The origin must be sculk, a vein, or air / a water source next to a
/// full-cube block. Runs `spread_rounds + growth_rounds` rounds; each adds
/// `charge_count` cursors of `amount_per_charge` (split into cursors of at
/// most 1000, at most 32 cursors), updates all cursors `spread_attempts`
/// times (blocks are converted only in the spread rounds) and clears them.
/// Then places a sculk catalyst at the origin when
/// `nextFloat() <= catalyst_chance` and the block below is a full cube, and
/// tries `extra_rare_growths` shriekers (`can_summon = true`) at random
/// offsets within 2 blocks on air above a sturdy top face. Returns `true`
/// once the origin check passes.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &SculkPatchConfig,
    origin: BlockPos,
) -> bool {
    if !can_spread_from(world, origin) {
        return false;
    }
    let mut cursors: Vec<Cursor> = Vec::with_capacity(MAX_CURSORS);
    let rounds = cfg.spread_rounds + cfg.growth_rounds;
    for round in 0..rounds {
        for _ in 0..cfg.charge_count {
            let mut charge = cfg.amount_per_charge;
            while charge > 0 {
                let c = charge.min(MAX_CHARGE);
                if cursors.len() < MAX_CURSORS {
                    cursors.push(Cursor {
                        pos: origin,
                        charge: c,
                        update_delay: 0,
                        decay_delay: 1,
                        facings: None,
                    });
                }
                charge -= c;
            }
        }
        let convert = round < cfg.spread_rounds;
        for _ in 0..cfg.spread_attempts {
            cursors.retain_mut(|cursor| {
                update_cursor(world, cursor, origin, rng, convert);
                cursor.charge > 0
            });
        }
        cursors.clear();
    }
    let below = origin.below();
    if rng.next_float() <= cfg.catalyst_chance && world.get_state(below).is_collision_full_cube() {
        world.set_state(origin, states::SCULK_CATALYST);
    }
    let extra = cfg.extra_rare_growths.sample(rng);
    for _ in 0..extra {
        let dx = rng.next_int_bounded(5) - 2;
        let dz = rng.next_int_bounded(5) - 2;
        let pos = origin.offset(dx, 0, dz);
        if world.get_state(pos).is_air()
            && world.get_state(pos.below()).is_face_sturdy(Direction::Up)
        {
            world.set_state(pos, states::SCULK_SHRIEKER.with_bool("can_summon", true));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    /// `sculk_patch_deep_dark`.
    const DEEP_DARK: SculkPatchConfig = SculkPatchConfig {
        charge_count: 10,
        amount_per_charge: 32,
        spread_attempts: 64,
        growth_rounds: 0,
        spread_rounds: 1,
        extra_rare_growths: IntProvider::Uniform { min: 1, max: 3 },
        catalyst_chance: 0.5,
    };

    fn deepslate_cave() -> TestWorld {
        TestWorld {
            fill: states::DEEPSLATE,
            ..TestWorld::flat(0)
        }
    }

    #[test]
    fn neighbour_table() {
        assert_eq!(NON_CORNER_NEIGHBOURS[0], [0, -1, -1]);
        assert_eq!(NON_CORNER_NEIGHBOURS[17], [0, 1, 1]);
        for [x, y, z] in NON_CORNER_NEIGHBOURS {
            assert!(x.abs() + y.abs() + z.abs() <= 2);
        }
    }

    #[test]
    fn spreads_sculk_on_floor() {
        let mut converted = 0;
        for seed in 0..8 {
            let mut world = deepslate_cave();
            let mut rng = FeatureRandom::new(seed);
            let origin = BlockPos::new(0, 0, 0);
            assert!(place(&mut world, &mut rng, &DEEP_DARK, origin));
            for (pos, state) in &world.blocks {
                let horizontal = ((pos.x * pos.x + pos.z * pos.z) as f64).sqrt();
                // Cursors die at 15; sculk and veins reach at most ~2.5 further.
                assert!(horizontal < 18.0, "{pos:?}");
                if state.is(b::SCULK) {
                    converted += 1;
                    assert!(pos.y < 0);
                }
                if state.is(b::SCULK_VEIN) {
                    assert!(Direction::ALL.iter().any(|d| has_face(*state, *d)));
                }
                if state.is(b::SCULK_SENSOR) || state.is(b::SCULK_SHRIEKER) {
                    assert_eq!(pos.y, 0);
                }
            }
        }
        assert!(converted > 0);
    }

    #[test]
    fn rejects_floating_or_solid_origin() {
        let mut world = deepslate_cave();
        let mut rng = FeatureRandom::new(1);
        assert!(!place(
            &mut world,
            &mut rng,
            &DEEP_DARK,
            BlockPos::new(0, 10, 0)
        ));
        assert!(!place(
            &mut world,
            &mut rng,
            &DEEP_DARK,
            BlockPos::new(0, -5, 0)
        ));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut world = deepslate_cave();
            let mut rng = FeatureRandom::new(seed);
            place(&mut world, &mut rng, &DEEP_DARK, BlockPos::new(2, 0, 3));
            let mut blocks: Vec<_> = world
                .blocks
                .into_iter()
                .map(|(p, s)| (p.x, p.y, p.z, s.0))
                .collect();
            blocks.sort();
            (blocks, rng.next_int())
        };
        assert_eq!(run(11), run(11));
    }

    #[test]
    fn decay_penalty_matches_float_formula() {
        // Distance 12 from the origin: (12 - 1)^2 / 23^2 * 0.5 * charge.
        let pos = BlockPos::new(12, 0, 0);
        let origin = BlockPos::new(0, 0, 0);
        assert_eq!(
            decay_penalty(pos, origin, 100),
            (100.0f32 * (121.0f32 / 529.0) * 0.5) as i32
        );
        // Never below 1.
        assert_eq!(decay_penalty(BlockPos::new(1, 0, 0), origin, 5), 1);
    }
}
