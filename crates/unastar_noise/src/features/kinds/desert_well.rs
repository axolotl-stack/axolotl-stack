//! `minecraft:desert_well`: the sandstone well with a water basin and two
//! suspicious sand blocks underneath.

use crate::features::blocks::{BlockPos, Direction};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::FeatureRandom;

/// Java `minecraft:desert_well` (`DesertWellFeature`).
///
/// Starts one block above the origin and drops through air (down to
/// `min_y + 2`); the block reached must be sand, and no column of the 5x5
/// area may have air both one and two blocks below it. Builds the 5x5x3
/// sandstone base, the water cross at the top with sand under it, the rim
/// with slabs at the four edge centres, the four pillars and the slab roof
/// with a sandstone centre. Then picks (`nextInt(5)` each) two of the centre
/// and its four neighbours (centre, east, south, west, north) and places
/// `suspicious_sand` one and two blocks below them.
///
/// The brushable block entity's loot table (`archaeology/desert_well`, seeded
/// with the position) is set by the runtime; this only writes the block state.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let mut pos = origin.above();
    while world.is_air(pos) && pos.y > world.min_y() + 2 {
        pos = pos.below();
    }
    if !world.is_block(pos, b::SAND) {
        return false;
    }
    for x in -2..=2 {
        for z in -2..=2 {
            if world.is_air(pos.offset(x, -1, z)) && world.is_air(pos.offset(x, -2, z)) {
                return false;
            }
        }
    }

    for y in -2..=0 {
        for x in -2..=2 {
            for z in -2..=2 {
                world.set_state(pos.offset(x, y, z), states::SANDSTONE);
            }
        }
    }
    world.set_state(pos, states::WATER);
    for dir in Direction::HORIZONTAL {
        world.set_state(pos.rel(dir), states::WATER);
    }
    let below = pos.below();
    world.set_state(below, states::SAND);
    for dir in Direction::HORIZONTAL {
        world.set_state(below.rel(dir), states::SAND);
    }
    for x in -2..=2 {
        for z in -2..=2 {
            if x == -2 || x == 2 || z == -2 || z == 2 {
                world.set_state(pos.offset(x, 1, z), states::SANDSTONE);
            }
        }
    }
    world.set_state(pos.offset(2, 1, 0), states::SANDSTONE_SLAB);
    world.set_state(pos.offset(-2, 1, 0), states::SANDSTONE_SLAB);
    world.set_state(pos.offset(0, 1, 2), states::SANDSTONE_SLAB);
    world.set_state(pos.offset(0, 1, -2), states::SANDSTONE_SLAB);
    for x in -1..=1 {
        for z in -1..=1 {
            let s = if x == 0 && z == 0 {
                states::SANDSTONE
            } else {
                states::SANDSTONE_SLAB
            };
            world.set_state(pos.offset(x, 4, z), s);
        }
    }
    for y in 1..=3 {
        world.set_state(pos.offset(-1, y, -1), states::SANDSTONE);
        world.set_state(pos.offset(-1, y, 1), states::SANDSTONE);
        world.set_state(pos.offset(1, y, -1), states::SANDSTONE);
        world.set_state(pos.offset(1, y, 1), states::SANDSTONE);
    }

    let candidates = [
        pos,
        pos.rel(Direction::East),
        pos.rel(Direction::South),
        pos.rel(Direction::West),
        pos.rel(Direction::North),
    ];
    let first = candidates[rng.next_int_bounded(candidates.len() as i32) as usize];
    world.set_state(first.offset(0, -1, 0), states::SUSPICIOUS_SAND);
    let second = candidates[rng.next_int_bounded(candidates.len() as i32) as usize];
    world.set_state(second.offset(0, -2, 0), states::SUSPICIOUS_SAND);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;

    fn desert() -> TestWorld {
        TestWorld {
            fill: states::SAND,
            ..TestWorld::flat(64)
        }
    }

    #[test]
    fn needs_sand() {
        let mut w = TestWorld::flat(64);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(&mut w, &mut rng, BlockPos::new(0, 70, 0)));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn fails_over_hollow_ground() {
        let mut w = desert();
        w.blocks.insert(BlockPos::new(2, 62, 2), states::AIR);
        w.blocks.insert(BlockPos::new(2, 61, 2), states::AIR);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(&mut w, &mut rng, BlockPos::new(0, 64, 0)));
    }

    #[test]
    fn builds_well() {
        let mut w = desert();
        let mut rng = FeatureRandom::new(9);
        assert!(place(&mut w, &mut rng, BlockPos::new(0, 64, 0)));
        // Surface sand is at y=63.
        assert_eq!(w.get_state(BlockPos::new(0, 63, 0)), states::WATER);
        assert_eq!(w.get_state(BlockPos::new(1, 63, 0)), states::WATER);
        assert_eq!(w.get_state(BlockPos::new(1, 63, 1)), states::SANDSTONE);
        assert_eq!(w.get_state(BlockPos::new(2, 64, 0)), states::SANDSTONE_SLAB);
        assert_eq!(w.get_state(BlockPos::new(2, 64, 2)), states::SANDSTONE);
        assert_eq!(w.get_state(BlockPos::new(0, 67, 0)), states::SANDSTONE);
        assert_eq!(w.get_state(BlockPos::new(1, 67, 0)), states::SANDSTONE_SLAB);
        assert_eq!(w.get_state(BlockPos::new(1, 66, 1)), states::SANDSTONE);
        let sus = w.count(states::SUSPICIOUS_SAND);
        assert_eq!(sus, 2);
        for (p, s) in &w.blocks {
            if *s == states::SUSPICIOUS_SAND {
                assert!(p.y == 62 || p.y == 61, "{p:?}");
                assert!(p.x.abs() + p.z.abs() <= 1, "{p:?}");
            }
        }
    }
}
