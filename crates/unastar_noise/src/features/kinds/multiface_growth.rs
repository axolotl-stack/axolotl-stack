//! `minecraft:multiface_growth` and the multiface spreading logic shared with
//! the sculk patch (`MultifaceSpreader`).
//!
//! Multiface blocks (glow lichen, sculk vein) carry one boolean property per
//! attached face, named after the direction (`down`, `up`, `north`, ...), plus
//! `waterlogged`.
//!
//! Approximation: Java's `MultifaceBlock.canAttachTo` tests whether the
//! neighbour's support shape *or* collision shape covers the touching face
//! fully. The block data only carries the sturdy-face flags (full support
//! face), so attachment uses [`JavaState::is_face_sturdy`] on the touching
//! face. The two agree for every full block and for the common partial blocks
//! decoration meets underground.
//!
//! Java also marks placed positions for chunk post-processing (a later shape
//! update that removes unsupported faces); decoration has no post-processing
//! pass here, and faces are only ever placed against sturdy neighbours, so
//! the marking has no effect on the generated blocks.

use crate::features::blocks::{BlockPos, Direction, JavaBlock, JavaState};
use crate::features::config::MultifaceGrowthConfig;
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, tags};
use crate::features::random::FeatureRandom;

/// Java `Util.shuffle(list, random)`: from the end, swap each slot `i - 1`
/// with `nextInt(i)`, for `i` from the length down to 2.
pub(crate) fn java_shuffle<T>(items: &mut [T], rng: &mut FeatureRandom) {
    let mut i = items.len();
    while i > 1 {
        let j = rng.next_int_bounded(i as i32) as usize;
        items.swap(i - 1, j);
        i -= 1;
    }
}

/// `Direction.allShuffled(random)`: a shuffled copy of `Direction.values()`.
pub(crate) fn all_directions_shuffled(rng: &mut FeatureRandom) -> [Direction; 6] {
    let mut dirs = Direction::ALL;
    java_shuffle(&mut dirs, rng);
    dirs
}

/// `MultifaceBlock.hasFace(state, dir)`: the face's boolean property is set.
#[inline]
pub(crate) fn has_face(state: JavaState, dir: Direction) -> bool {
    state.value(dir.name()) == Some("true")
}

/// `MultifaceBlock.canAttachTo(level, dir, neighbourPos, neighbourState)`:
/// the neighbour in direction `dir` offers a full face towards the block.
#[inline]
pub(crate) fn can_attach_to(neighbour: JavaState, dir: Direction) -> bool {
    neighbour.is_face_sturdy(dir.opposite())
}

#[inline]
fn same_axis(a: Direction, b: Direction) -> bool {
    a.axis() == b.axis()
}

/// `MultifaceSpreader.SpreadType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

const DEFAULT_SPREAD_ORDER: [SpreadType; 3] = [
    SpreadType::SamePosition,
    SpreadType::SamePlane,
    SpreadType::WrapAround,
];

impl SpreadType {
    /// `getSpreadPos(pos, face, spreadDirection)`: target position and the
    /// face the new growth attaches to.
    #[inline]
    fn spread_pos(self, pos: BlockPos, face: Direction, from: Direction) -> (BlockPos, Direction) {
        match self {
            SpreadType::SamePosition => (pos, face),
            SpreadType::SamePlane => (pos.rel(face), from),
            SpreadType::WrapAround => (pos.rel(face).rel(from), face.opposite()),
        }
    }
}

/// A `MultifaceSpreader` configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spreader {
    /// `DefaultSpreaderConfig(block)` (glow lichen and other multiface blocks).
    Default(JavaBlock),
    /// Sculk vein's `getSpreader()`: all three spread types.
    SculkVein,
    /// Sculk vein's `getSameSpaceSpreader()`: `SAME_POSITION` only.
    SculkVeinSameSpace,
}

impl Spreader {
    /// `MultifaceSpreadeableBlock.getSpreader()` of `block`.
    pub(crate) fn of(block: JavaBlock) -> Spreader {
        if block == b::SCULK_VEIN {
            Spreader::SculkVein
        } else {
            Spreader::Default(block)
        }
    }

    #[inline]
    fn block(self) -> JavaBlock {
        match self {
            Spreader::Default(block) => block,
            Spreader::SculkVein | Spreader::SculkVeinSameSpace => b::SCULK_VEIN,
        }
    }

    #[inline]
    fn spread_types(self) -> &'static [SpreadType] {
        match self {
            Spreader::Default(_) | Spreader::SculkVein => &DEFAULT_SPREAD_ORDER,
            Spreader::SculkVeinSameSpace => &[SpreadType::SamePosition],
        }
    }

    /// `isOtherBlockValidAsSource`: sculk vein spreads from any non-vein block
    /// (a fresh sculk block, the cursor's air); the default config never does.
    #[inline]
    fn other_block_valid_as_source(self, state: JavaState) -> bool {
        match self {
            Spreader::Default(_) => false,
            Spreader::SculkVein | Spreader::SculkVeinSameSpace => !state.is(b::SCULK_VEIN),
        }
    }

    /// `stateCanBeReplaced(level, pos, spreadPos, direction, state)`.
    fn state_can_be_replaced<W: FeatureWorld + ?Sized>(
        self,
        world: &mut W,
        pos: BlockPos,
        spread_pos: BlockPos,
        dir: Direction,
        state: JavaState,
    ) -> bool {
        let default_replaceable = |state: JavaState, block: JavaBlock| {
            state.is_air() || state.is(block) || (state.is(b::WATER) && state.fluid().source)
        };
        match self {
            Spreader::Default(block) => default_replaceable(state, block),
            Spreader::SculkVein | Spreader::SculkVeinSameSpace => {
                let attached = world.get_state(spread_pos.rel(dir));
                if attached.is(b::SCULK)
                    || attached.is(b::SCULK_CATALYST)
                    || attached.is(b::MOVING_PISTON)
                {
                    return false;
                }
                let manhattan = (pos.x - spread_pos.x).abs()
                    + (pos.y - spread_pos.y).abs()
                    + (pos.z - spread_pos.z).abs();
                if manhattan == 2 {
                    let corner = pos.rel(dir.opposite());
                    if world.get_state(corner).is_face_sturdy(dir) {
                        return false;
                    }
                }
                let fluid = state.fluid();
                if !fluid.is_empty() && !fluid.is_water() {
                    return false;
                }
                if state.is_in(tags::FIRE) {
                    return false;
                }
                state.can_be_replaced() || default_replaceable(state, b::SCULK_VEIN)
            }
        }
    }

    /// `canSpreadInto(level, pos, spreadPos)`.
    fn can_spread_into<W: FeatureWorld + ?Sized>(
        self,
        world: &mut W,
        pos: BlockPos,
        target: (BlockPos, Direction),
    ) -> bool {
        let (spread_pos, face) = target;
        let state = world.get_state(spread_pos);
        self.state_can_be_replaced(world, pos, spread_pos, face, state)
            && is_valid_state_for_placement(world, self.block(), state, spread_pos, face)
    }

    /// `getSpreadFromFaceTowardDirection` followed by `spreadToFace`: grows
    /// from the `from` face of `state` at `pos` towards `face`. Returns
    /// whether a block was placed.
    fn spread_from_face_toward_direction<W: FeatureWorld + ?Sized>(
        self,
        world: &mut W,
        state: JavaState,
        pos: BlockPos,
        from: Direction,
        face: Direction,
    ) -> bool {
        if same_axis(face, from) {
            return false;
        }
        if !(self.other_block_valid_as_source(state)
            || (has_face(state, from) && !has_face(state, face)))
        {
            return false;
        }
        for &kind in self.spread_types() {
            let target = kind.spread_pos(pos, face, from);
            if self.can_spread_into(world, pos, target) {
                // `spreadToFace` -> `placeBlock`.
                let (spread_pos, spread_face) = target;
                let current = world.get_state(spread_pos);
                return match state_for_placement(
                    world,
                    self.block(),
                    current,
                    spread_pos,
                    spread_face,
                ) {
                    Some(new_state) => world.set_state(spread_pos, new_state),
                    None => false,
                };
            }
        }
        false
    }

    /// `spreadAll(state, level, pos, markForPostprocessing)`: the number of
    /// faces grown from every face of `state` towards every direction.
    pub(crate) fn spread_all<W: FeatureWorld + ?Sized>(
        self,
        world: &mut W,
        state: JavaState,
        pos: BlockPos,
    ) -> u64 {
        let mut count = 0;
        for from in Direction::ALL {
            for face in Direction::ALL {
                if self.spread_from_face_toward_direction(world, state, pos, from, face) {
                    count += 1;
                }
            }
        }
        count
    }

    /// `spreadFromFaceTowardRandomDirection`: shuffles all six directions
    /// (always five draws) and stops at the first successful spread.
    pub(crate) fn spread_from_face_toward_random_direction<W: FeatureWorld + ?Sized>(
        self,
        world: &mut W,
        state: JavaState,
        pos: BlockPos,
        from: Direction,
        rng: &mut FeatureRandom,
    ) -> bool {
        for face in all_directions_shuffled(rng) {
            if self.spread_from_face_toward_direction(world, state, pos, from, face) {
                return true;
            }
        }
        false
    }
}

/// `MultifaceBlock.isValidStateForPlacement(level, state, pos, dir)`.
pub(crate) fn is_valid_state_for_placement<W: FeatureWorld + ?Sized>(
    world: &mut W,
    block: JavaBlock,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
) -> bool {
    if state.is(block) && has_face(state, dir) {
        return false;
    }
    let neighbour = world.get_state(pos.rel(dir));
    can_attach_to(neighbour, dir)
}

/// `MultifaceBlock.getStateForPlacement(current, level, pos, dir)`: adds the
/// face to an existing block of this type, or starts from the default state,
/// waterlogged when replacing a water source.
pub(crate) fn state_for_placement<W: FeatureWorld + ?Sized>(
    world: &mut W,
    block: JavaBlock,
    current: JavaState,
    pos: BlockPos,
    dir: Direction,
) -> Option<JavaState> {
    if !is_valid_state_for_placement(world, block, current, pos, dir) {
        return None;
    }
    let base = if current.is(block) {
        current
    } else {
        let default = block.default_state();
        let fluid = current.fluid();
        if default.has("waterlogged") && fluid.is_water() && fluid.source {
            default.with_bool("waterlogged", true)
        } else {
            default
        }
    };
    Some(base.with_bool(dir.name(), true))
}

/// `isAirOrWater`: air or the water block (any level).
#[inline]
fn is_air_or_water(state: JavaState) -> bool {
    state.is_air() || state.is(b::WATER)
}

/// Fixed buffer for the shuffled direction lists (at most six entries).
#[derive(Clone, Copy)]
struct DirList {
    dirs: [Direction; 6],
    len: usize,
}

impl DirList {
    #[inline]
    fn as_slice(&self) -> &[Direction] {
        &self.dirs[..self.len]
    }
}

/// `getShuffledDirectionsExcept(random, except)` (or all valid directions
/// with `except = None`): a filtered copy of `valid_directions`, shuffled.
fn shuffled_directions(
    cfg: &MultifaceGrowthConfig,
    except: Option<Direction>,
    rng: &mut FeatureRandom,
) -> DirList {
    let mut list = DirList {
        dirs: [Direction::Down; 6],
        len: 0,
    };
    for &dir in cfg.valid_directions {
        if Some(dir) != except {
            list.dirs[list.len] = dir;
            list.len += 1;
        }
    }
    java_shuffle(&mut list.dirs[..list.len], rng);
    list
}

/// `MultifaceGrowthFeature.placeGrowthIfPossible`: attaches to the first
/// direction (in list order) whose neighbour is in `can_be_placed_on`. Gives
/// up if that face cannot hold the block; after placing, spreads with
/// `chance_of_spreading` (one float, then the spreader's draws).
fn place_growth_if_possible<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    state: JavaState,
    cfg: &MultifaceGrowthConfig,
    rng: &mut FeatureRandom,
    dirs: &[Direction],
) -> bool {
    for &dir in dirs {
        if !world.get_state(pos.rel(dir)).is_in(cfg.can_be_placed_on) {
            continue;
        }
        let Some(placed) = state_for_placement(world, cfg.block, state, pos, dir) else {
            return false;
        };
        world.set_state(pos, placed);
        if rng.next_float() < cfg.chance_of_spreading {
            Spreader::of(cfg.block)
                .spread_from_face_toward_random_direction(world, placed, pos, dir, rng);
        }
        return true;
    }
    false
}

/// Java `minecraft:multiface_growth` (`MultifaceGrowthFeature`), used for glow
/// lichen and sculk veins.
///
/// The origin must be air or water. Tries the origin with the shuffled valid
/// directions; otherwise, for each of those directions, draws a fresh
/// shuffled list without the opposite direction and searches `search_range`
/// times. Like vanilla, every search step looks at the block directly next to
/// the origin (the search position is recomputed from the origin, not
/// advanced), stopping at a block that is neither air, water nor the growth
/// block. Returns whether a growth was placed.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &MultifaceGrowthConfig,
    origin: BlockPos,
) -> bool {
    let origin_state = world.get_state(origin);
    if !is_air_or_water(origin_state) {
        return false;
    }
    let dirs = shuffled_directions(cfg, None, rng);
    if place_growth_if_possible(world, origin, origin_state, cfg, rng, dirs.as_slice()) {
        return true;
    }
    for &dir in dirs.as_slice() {
        let others = shuffled_directions(cfg, Some(dir.opposite()), rng);
        for _ in 0..cfg.search_range {
            let pos = origin.rel(dir);
            let state = world.get_state(pos);
            if !is_air_or_water(state) && !state.is(cfg.block) {
                break;
            }
            if place_growth_if_possible(world, pos, state, cfg, rng, others.as_slice()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;

    const ALL_DIRS: [Direction; 6] = [
        Direction::Up,
        Direction::Down,
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    const LICHEN: MultifaceGrowthConfig = MultifaceGrowthConfig {
        block: b::GLOW_LICHEN,
        search_range: 20,
        can_place_on_floor: true,
        can_place_on_ceiling: true,
        can_place_on_wall: true,
        chance_of_spreading: 0.5,
        can_be_placed_on: tags::BASE_STONE_OVERWORLD,
        valid_directions: &ALL_DIRS,
    };

    fn faces(state: JavaState) -> usize {
        Direction::ALL
            .iter()
            .filter(|d| has_face(state, **d))
            .count()
    }

    #[test]
    fn shuffle_matches_util_shuffle_draw_count() {
        let mut a = FeatureRandom::new(5);
        let mut dirs = Direction::ALL;
        java_shuffle(&mut dirs, &mut a);
        let mut b = FeatureRandom::new(5);
        for i in (2..=6).rev() {
            b.next_int_bounded(i);
        }
        assert_eq!(a.next_int(), b.next_int());
        let mut sorted = dirs.map(|d| d as u8);
        sorted.sort();
        assert_eq!(sorted, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn places_on_floor_in_cave() {
        let mut world = TestWorld::flat(10);
        let origin = BlockPos::new(0, 10, 0);
        let mut rng = FeatureRandom::new(1);
        assert!(place(&mut world, &mut rng, &LICHEN, origin));
        let placed: Vec<_> = world
            .blocks
            .iter()
            .filter(|(_, s)| s.is(b::GLOW_LICHEN))
            .map(|(p, s)| (*p, *s))
            .collect();
        assert!(!placed.is_empty());
        for (pos, state) in placed {
            assert!(faces(state) >= 1);
            for dir in Direction::ALL {
                if has_face(state, dir) {
                    assert!(world.get_state(pos.rel(dir)).is_face_sturdy(dir.opposite()));
                }
            }
            assert_eq!(state.value("waterlogged"), Some("false"));
        }
    }

    #[test]
    fn waterlogged_in_water() {
        let mut world = TestWorld::flat(10);
        let origin = BlockPos::new(0, 10, 0);
        world.blocks.insert(origin, states::WATER);
        let mut rng = FeatureRandom::new(3);
        let cfg = MultifaceGrowthConfig {
            chance_of_spreading: 0.0,
            ..LICHEN
        };
        assert!(place(&mut world, &mut rng, &cfg, origin));
        let state = world.get_state(origin);
        assert!(state.is(b::GLOW_LICHEN));
        assert!(has_face(state, Direction::Down));
        assert_eq!(state.value("waterlogged"), Some("true"));
    }

    #[test]
    fn rejects_solid_origin_and_unsupported_air() {
        let mut world = TestWorld::flat(10);
        let mut rng = FeatureRandom::new(0);
        assert!(!place(
            &mut world,
            &mut rng,
            &LICHEN,
            BlockPos::new(0, 5, 0)
        ));
        // Floating in open air: nothing to attach to.
        assert!(!place(
            &mut world,
            &mut rng,
            &LICHEN,
            BlockPos::new(0, 40, 0)
        ));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut world = TestWorld::flat(10);
            let mut rng = FeatureRandom::new(seed);
            place(&mut world, &mut rng, &LICHEN, BlockPos::new(3, 10, -2));
            let mut blocks: Vec<_> = world
                .blocks
                .into_iter()
                .map(|(p, s)| (p.x, p.y, p.z, s.0))
                .collect();
            blocks.sort();
            (blocks, rng.next_int())
        };
        assert_eq!(run(42), run(42));
    }

    #[test]
    fn sculk_vein_spreader_does_not_cover_sculk() {
        let mut world = TestWorld::flat(10);
        let pos = BlockPos::new(0, 10, 0);
        // Sculk floor next to stone: veins may only attach to non-sculk blocks.
        world.blocks.insert(pos.below(), states::SCULK);
        let n = Spreader::SculkVeinSameSpace.spread_all(&mut world, states::AIR, pos);
        assert_eq!(n, 0);
        world.blocks.insert(pos.below(), states::STONE);
        let n = Spreader::SculkVeinSameSpace.spread_all(&mut world, states::AIR, pos);
        assert!(n > 0);
        let state = world.get_state(pos);
        assert!(state.is(b::SCULK_VEIN) && has_face(state, Direction::Down));
    }
}
