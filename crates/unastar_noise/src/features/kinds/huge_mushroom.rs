//! `minecraft:huge_brown_mushroom` and `minecraft:huge_red_mushroom`
//! (`AbstractHugeMushroomFeature` and its two cap shapes).

use crate::features::blocks::{BlockPos, JavaState};
use crate::features::config::HugeMushroomConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::tags;
use crate::features::random::FeatureRandom;

/// Which cap shape is grown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Brown,
    Red,
}

impl Kind {
    /// `getTreeRadiusForHeight(-1, -1, foliageRadius, y)` as used by the
    /// position check: brown caps clear `foliage_radius` above y 3; the red
    /// shape's rule never matches with height -1, so only the column is
    /// checked.
    #[inline]
    fn clearance_radius(self, foliage_radius: i32, y: i32) -> i32 {
        match self {
            Kind::Brown => {
                if y <= 3 {
                    0
                } else {
                    foliage_radius
                }
            }
            Kind::Red => 0,
        }
    }
}

/// `getTreeHeight`: `nextInt(3) + 4`, doubled with `nextInt(12) == 0`.
#[inline]
fn tree_height(rng: &mut FeatureRandom) -> i32 {
    let mut h = rng.next_int_bounded(3) + 4;
    if rng.next_int_bounded(12) == 0 {
        h *= 2;
    }
    h
}

/// `placeMushroomBlock`: only replaces air and `#replaceable_by_mushrooms`.
#[inline]
fn place_block<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos, state: JavaState) {
    let current = world.get_state(pos);
    if current.is_air() || current.is_in(tags::REPLACEABLE_BY_MUSHROOMS) {
        world.set_state(pos, state);
    }
}

/// `isValidPosition`: inside the build height with room for the cap, on dirt
/// or `#mushroom_grow_block`, and air or leaves in the clearance volume.
fn is_valid_position<W: FeatureWorld + ?Sized>(
    world: &mut W,
    kind: Kind,
    origin: BlockPos,
    height: i32,
    cfg: &HugeMushroomConfig,
) -> bool {
    let y = origin.y;
    if y < world.min_y() + 1 || y + height + 1 > world.max_y() {
        return false;
    }
    let below = world.get_state(origin.below());
    if !below.is_in(tags::DIRT) && !below.is_in(tags::MUSHROOM_GROW_BLOCK) {
        return false;
    }
    for dy in 0..=height {
        let r = kind.clearance_radius(cfg.foliage_radius, dy);
        for dx in -r..=r {
            for dz in -r..=r {
                let s = world.get_state(origin.offset(dx, dy, dz));
                if !s.is_air() && !s.is_in(tags::LEAVES) {
                    return false;
                }
            }
        }
    }
    true
}

/// Sets the four horizontal face properties when the cap state has them.
#[inline]
fn with_sides(state: JavaState, west: bool, east: bool, north: bool, south: bool) -> JavaState {
    state
        .with_bool("west", west)
        .with_bool("east", east)
        .with_bool("north", north)
        .with_bool("south", south)
}

#[inline]
fn has_sides(state: JavaState) -> bool {
    state.has("west") && state.has("east") && state.has("north") && state.has("south")
}

/// Brown cap: one flat layer at `height`, a square of `foliage_radius`
/// without its corners; the outer ring and the blocks next to the cut
/// corners show their outside faces.
fn make_brown_cap<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
    height: i32,
    cfg: &HugeMushroomConfig,
) {
    let r = cfg.foliage_radius;
    for dx in -r..=r {
        for dz in -r..=r {
            let min_x = dx == -r;
            let max_x = dx == r;
            let min_z = dz == -r;
            let max_z = dz == r;
            let x_edge = min_x || max_x;
            let z_edge = min_z || max_z;
            if x_edge && z_edge {
                continue;
            }
            let pos = origin.offset(dx, height, dz);
            let west = min_x || (z_edge && dx == 1 - r);
            let east = max_x || (z_edge && dx == r - 1);
            let north = min_z || (x_edge && dz == 1 - r);
            let south = max_z || (x_edge && dz == r - 1);
            // Java samples the provider at the feature origin.
            let mut state = provide_state(&cfg.cap_provider, rng, origin);
            if has_sides(state) {
                state = with_sides(state, west, east, north, south);
            }
            place_block(world, pos, state);
        }
    }
}

/// Red cap: walls of radius `foliage_radius` (without corners) from
/// `height - 3` to `height - 1` and a roof of radius `foliage_radius - 1` at
/// `height`; faces point outwards beyond `foliage_radius - 2`, `up` on the
/// top two layers.
fn make_red_cap<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
    height: i32,
    cfg: &HugeMushroomConfig,
) {
    for dy in height - 3..=height {
        let r = if dy < height {
            cfg.foliage_radius
        } else {
            cfg.foliage_radius - 1
        };
        let inner = cfg.foliage_radius - 2;
        for dx in -r..=r {
            for dz in -r..=r {
                let x_edge = dx == -r || dx == r;
                let z_edge = dz == -r || dz == r;
                if !(dy >= height || x_edge != z_edge) {
                    continue;
                }
                let pos = origin.offset(dx, dy, dz);
                let mut state = provide_state(&cfg.cap_provider, rng, origin);
                if has_sides(state) && state.has("up") {
                    state = with_sides(state, dx < -inner, dx > inner, dz < -inner, dz > inner)
                        .with_bool("up", dy >= height - 1);
                }
                place_block(world, pos, state);
            }
        }
    }
}

fn place_kind<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeMushroomConfig,
    origin: BlockPos,
    kind: Kind,
) -> bool {
    let height = tree_height(rng);
    if !is_valid_position(world, kind, origin, height, cfg) {
        return false;
    }
    match kind {
        Kind::Brown => make_brown_cap(world, rng, origin, height, cfg),
        Kind::Red => make_red_cap(world, rng, origin, height, cfg),
    }
    // `placeTrunk`: the provider is sampled at the origin for every block.
    for i in 0..height {
        let state = provide_state(&cfg.stem_provider, rng, origin);
        place_block(world, origin.offset(0, i, 0), state);
    }
    true
}

/// Java `minecraft:huge_brown_mushroom` (`HugeBrownMushroomFeature`).
///
/// Height `nextInt(3) + 4`, doubled when `nextInt(12) == 0`. Needs dirt or
/// `#mushroom_grow_block` below, air or leaves in the stem column up to the
/// cap and in the `foliage_radius` square above y 3. Places the flat cap,
/// then the stem (`height` blocks). Blocks only replace air and
/// `#replaceable_by_mushrooms`.
pub fn place_brown<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeMushroomConfig,
    origin: BlockPos,
) -> bool {
    place_kind(world, rng, cfg, origin, Kind::Brown)
}

/// Java `minecraft:huge_red_mushroom` (`HugeRedMushroomFeature`).
///
/// Same height and ground rules as [`place_brown`], but only the stem column
/// must be clear; places the dome cap (sides from `height - 3`, roof at
/// `height`), then the stem.
pub fn place_red<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeMushroomConfig,
    origin: BlockPos,
) -> bool {
    place_kind(world, rng, cfg, origin, Kind::Red)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::java_blocks::{blocks as b, states};
    use crate::features::test_world::TestWorld;

    fn stem() -> JavaState {
        states::MUSHROOM_STEM
            .with_bool("up", false)
            .with_bool("down", false)
    }

    fn brown_cfg() -> HugeMushroomConfig {
        HugeMushroomConfig {
            cap_provider: StateProvider::Simple(
                states::BROWN_MUSHROOM_BLOCK.with_bool("down", false),
            ),
            stem_provider: StateProvider::Simple(stem()),
            foliage_radius: 3,
        }
    }

    fn red_cfg() -> HugeMushroomConfig {
        HugeMushroomConfig {
            cap_provider: StateProvider::Simple(
                states::RED_MUSHROOM_BLOCK.with_bool("down", false),
            ),
            stem_provider: StateProvider::Simple(stem()),
            foliage_radius: 2,
        }
    }

    fn grass_world() -> TestWorld {
        let mut world = TestWorld::flat(10);
        world
            .blocks
            .insert(BlockPos::new(0, 9, 0), states::GRASS_BLOCK);
        world
    }

    fn stem_height(world: &mut TestWorld) -> i32 {
        let mut h = 0;
        while world
            .get_state(BlockPos::new(0, 10 + h, 0))
            .is(b::MUSHROOM_STEM)
        {
            h += 1;
        }
        h
    }

    #[test]
    fn brown_shape() {
        for seed in 0..40 {
            let mut world = grass_world();
            let mut rng = FeatureRandom::new(seed);
            let origin = BlockPos::new(0, 10, 0);
            assert!(place_brown(&mut world, &mut rng, &brown_cfg(), origin));
            let h = stem_height(&mut world);
            assert!(
                (4..=6).contains(&h) || [8, 10, 12].contains(&h),
                "height {h}"
            );
            let caps: Vec<_> = world
                .blocks
                .iter()
                .filter(|(_, s)| s.is(b::BROWN_MUSHROOM_BLOCK))
                .map(|(p, s)| (*p, *s))
                .collect();
            assert_eq!(caps.len(), 7 * 7 - 4);
            for (pos, state) in caps {
                assert_eq!(pos.y, 10 + h);
                assert_eq!(
                    state.value("west") == Some("true"),
                    pos.x == -3 || (pos.x == -2 && pos.z.abs() == 3)
                );
                assert_eq!(state.value("up"), Some("true"));
            }
        }
    }

    #[test]
    fn red_shape() {
        for seed in 0..40 {
            let mut world = grass_world();
            let mut rng = FeatureRandom::new(seed);
            let origin = BlockPos::new(0, 10, 0);
            assert!(place_red(&mut world, &mut rng, &red_cfg(), origin));
            let h = stem_height(&mut world);
            let caps: Vec<_> = world
                .blocks
                .iter()
                .filter(|(_, s)| s.is(b::RED_MUSHROOM_BLOCK))
                .map(|(p, s)| (*p, *s))
                .collect();
            // Three wall rings of 12 (5x5 border minus corners) and a 3x3 roof.
            assert_eq!(caps.len(), 3 * 12 + 9);
            for (pos, state) in caps {
                let dy = pos.y - 10;
                assert!(dy >= h - 3 && dy <= h);
                assert_eq!(state.value("up") == Some("true"), dy >= h - 1);
                assert_eq!(state.value("east") == Some("true"), pos.x > 0);
            }
            // The stem stops below the roof, which is part of the cap.
            assert!(
                world
                    .get_state(BlockPos::new(0, 10 + h, 0))
                    .is(b::RED_MUSHROOM_BLOCK)
            );
        }
    }

    #[test]
    fn rejects_bad_ground_and_obstruction() {
        let mut world = TestWorld::flat(10);
        let mut rng = FeatureRandom::new(1);
        // Stone is neither dirt nor mushroom_grow_block.
        assert!(!place_brown(
            &mut world,
            &mut rng,
            &brown_cfg(),
            BlockPos::new(0, 10, 0)
        ));
        let mut world = grass_world();
        world.blocks.insert(BlockPos::new(3, 16, 0), states::STONE);
        world.blocks.insert(BlockPos::new(0, 14, 0), states::STONE);
        let mut rng = FeatureRandom::new(1);
        assert!(!place_brown(
            &mut world,
            &mut rng,
            &brown_cfg(),
            BlockPos::new(0, 10, 0)
        ));
        assert!(!place_red(
            &mut world,
            &mut rng,
            &red_cfg(),
            BlockPos::new(0, 10, 0)
        ));
        assert_eq!(world.blocks.len(), 3);
    }

    #[test]
    fn rejects_near_build_limit() {
        let mut world = TestWorld::flat(10);
        let origin = BlockPos::new(0, 318, 0);
        world.blocks.insert(origin.below(), states::DIRT);
        let mut rng = FeatureRandom::new(0);
        // y + height + 1 > 320 for every height >= 4.
        assert!(!place_red(&mut world, &mut rng, &red_cfg(), origin));
    }
}
