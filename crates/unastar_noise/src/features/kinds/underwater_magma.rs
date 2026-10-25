//! `minecraft:underwater_magma`: magma blocks scattered over the floor below a
//! water column.

use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::UnderwaterMagmaConfig;
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::FeatureRandom;

/// Java `minecraft:underwater_magma` (`UnderwaterMagmaFeature`).
///
/// The origin must be water. Java scans the column down through water for at
/// most `floor_search_range - 1` steps (a `Column.scan` whose floor edge is any
/// non-water block); without a floor the feature fails. It then walks the cube
/// of radius `placement_radius_around_floor` around the floor position (x
/// fastest, then y, then z), drawing one `nextFloat` per position and, when it
/// passes the probability, placing magma if the position, the block below and
/// the four horizontal neighbours are all neither water nor air (the block
/// above is not checked, so the exposed floor surface qualifies). Earlier
/// placements are visible to later checks, as in Java's lazy stream.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &UnderwaterMagmaConfig,
    origin: BlockPos,
) -> bool {
    let Some(floor_y) = floor_y(world, origin, cfg.floor_search_range) else {
        return false;
    };
    let floor = origin.with_y(floor_y);
    let r = cfg.placement_radius_around_floor;
    let mut placed = false;
    for z in floor.z - r..=floor.z + r {
        for y in floor.y - r..=floor.y + r {
            for x in floor.x - r..=floor.x + r {
                if rng.next_float() >= cfg.placement_probability_per_valid_position {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                if is_valid_placement(world, pos) {
                    world.set_state(pos, states::MAGMA_BLOCK);
                    placed = true;
                }
            }
        }
    }
    placed
}

/// Floor of `Column.scan(origin, range, is(water), !is(water))`: the first
/// non-water Y below the origin within `range - 1` steps.
fn floor_y<W: FeatureWorld + ?Sized>(world: &mut W, origin: BlockPos, range: i32) -> Option<i32> {
    if !world.is_block(origin, b::WATER) {
        return None;
    }
    let mut pos = origin;
    let mut i = 1;
    while i < range && world.is_block(pos, b::WATER) {
        pos = pos.below();
        i += 1;
    }
    (!world.is_block(pos, b::WATER)).then_some(pos.y)
}

fn is_valid_placement<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    if is_water_or_air(world, pos) || is_water_or_air(world, pos.below()) {
        return false;
    }
    for dir in Direction::HORIZONTAL {
        if is_water_or_air(world, pos.rel(dir)) {
            return false;
        }
    }
    true
}

#[inline]
fn is_water_or_air<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let s = world.get_state(pos);
    s.is(b::WATER) || s.is_air()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;

    const CFG: UnderwaterMagmaConfig = UnderwaterMagmaConfig {
        floor_search_range: 5,
        placement_radius_around_floor: 1,
        placement_probability_per_valid_position: 0.5,
    };

    /// Stone below y=40, water 40..=50.
    fn ocean() -> TestWorld {
        let mut w = TestWorld::flat(40);
        for x in -4..=4 {
            for z in -4..=4 {
                for y in 40..=50 {
                    w.blocks.insert(BlockPos::new(x, y, z), states::WATER);
                }
            }
        }
        w
    }

    #[test]
    fn fails_outside_water() {
        let mut w = TestWorld::flat(40);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(&mut w, &mut rng, &CFG, BlockPos::new(0, 45, 0)));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn fails_when_floor_out_of_range() {
        let mut w = ocean();
        let mut rng = FeatureRandom::new(1);
        // Floor is 10 blocks down but the scan only moves 4.
        assert!(!place(&mut w, &mut rng, &CFG, BlockPos::new(0, 50, 0)));
        assert_eq!(w.count(states::MAGMA_BLOCK), 0);
    }

    #[test]
    fn magma_only_where_hidden_from_water() {
        let with_hole = || {
            let mut w = ocean();
            // A water pocket in the floor makes its neighbours invalid.
            w.blocks.insert(BlockPos::new(1, 38, 0), states::WATER);
            w
        };
        let mut total = 0;
        for seed in 0..20 {
            let mut w = with_hole();
            let mut reference = with_hole();
            let mut rng = FeatureRandom::new(seed);
            place(&mut w, &mut rng, &CFG, BlockPos::new(0, 42, 0));
            for (pos, s) in &w.blocks {
                if *s != states::MAGMA_BLOCK {
                    continue;
                }
                total += 1;
                // Floor is y=39: the cube spans 38..=40 and y=40 is water.
                assert!(pos.y == 38 || pos.y == 39, "magma at {pos:?}");
                assert!(!is_water_or_air(&mut reference, pos.below()), "{pos:?}");
                for dir in Direction::HORIZONTAL {
                    assert!(!is_water_or_air(&mut reference, pos.rel(dir)), "{pos:?}");
                }
            }
        }
        assert!(total > 0);
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut w = ocean();
            let mut rng = FeatureRandom::new(seed);
            let r = place(&mut w, &mut rng, &CFG, BlockPos::new(0, 42, 0));
            (r, w.count(states::MAGMA_BLOCK), rng.next_int())
        };
        assert_eq!(run(7), run(7));
    }
}
