//! `minecraft:bamboo`.

use crate::features::blocks::{BlockPos, Heightmap, JavaState};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{states, tags};
use crate::features::random::FeatureRandom;
use crate::features::survival::can_survive;

/// `BambooFeature.BAMBOO_TRUNK`: age 1, no leaves, stage 0.
fn trunk() -> JavaState {
    states::BAMBOO
        .with_int("age", 1)
        .with("leaves", "none")
        .with_int("stage", 0)
}

/// Java `minecraft:bamboo` (`BambooFeature`, `ProbabilityFeatureConfiguration`).
///
/// Only acts when the origin is air and bamboo survives there (returns `true`
/// as soon as the origin is air, `false` otherwise). Draws the height
/// `nextInt(12) + 5`, then on `nextFloat() < probability` a podzol disk of
/// radius `nextInt(4) + 1` that turns the top `WORLD_SURFACE` block of every
/// column within the circle into podzol when it is dirt-tagged. The stalk
/// grows up while the position is air, at most `height` blocks; if it reached
/// at least 3 above the origin, the block after the stalk becomes a large-leaf
/// stage-1 top, the two below it large and small leaves.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    probability: f32,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) {
        return false;
    }
    if can_survive(world, states::BAMBOO, origin) {
        let height = rng.next_int_bounded(12) + 5;
        if rng.next_float() < probability {
            let r = rng.next_int_bounded(4) + 1;
            for x in origin.x - r..=origin.x + r {
                for z in origin.z - r..=origin.z + r {
                    let dx = x - origin.x;
                    let dz = z - origin.z;
                    if dx * dx + dz * dz <= r * r {
                        let y = world.get_height(Heightmap::WorldSurface, x, z) - 1;
                        let pos = BlockPos::new(x, y, z);
                        if world.get_state(pos).is_in(tags::DIRT) {
                            world.set_state(pos, states::PODZOL);
                        }
                    }
                }
            }
        }
        let trunk = trunk();
        let mut pos = origin;
        let mut placed = 0;
        while placed < height && world.is_air(pos) {
            world.set_state(pos, trunk);
            pos = pos.above();
            placed += 1;
        }
        if pos.y - origin.y >= 3 {
            let top_large = trunk.with("leaves", "large");
            world.set_state(pos, top_large.with_int("stage", 1));
            world.set_state(pos.below(), top_large);
            world.set_state(pos.offset(0, -2, 0), trunk.with("leaves", "small"));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::blocks as b;
    use crate::features::test_world::TestWorld;

    fn grass_world() -> TestWorld {
        let mut w = TestWorld::flat(64);
        w.fill = states::GRASS_BLOCK;
        w
    }

    #[test]
    fn stalk_height_and_leaves() {
        let origin = BlockPos::new(0, 64, 0);
        for seed in 0..32 {
            let mut world = grass_world();
            let mut rng = FeatureRandom::new(seed);
            assert!(place(&mut world, &mut rng, 0.0, origin));
            let mut top = origin.y;
            while world.get_state(BlockPos::new(0, top, 0)).is(b::BAMBOO) {
                top += 1;
            }
            // Stalk of 5..=16 trunk blocks plus the final large top.
            let len = top - origin.y;
            assert!((6..=17).contains(&len), "seed {seed}: {len}");
            let at = |w: &mut TestWorld, y: i32| w.get_state(BlockPos::new(0, y, 0));
            assert_eq!(at(&mut world, top - 1).value("leaves"), Some("large"));
            assert_eq!(at(&mut world, top - 1).int_value("stage"), Some(1));
            assert_eq!(at(&mut world, top - 2).value("leaves"), Some("large"));
            assert_eq!(at(&mut world, top - 3).value("leaves"), Some("small"));
            assert_eq!(at(&mut world, top - 4), trunk());
            assert_eq!(world.count(states::PODZOL), 0);
        }
    }

    #[test]
    fn podzol_disk_with_full_probability() {
        let origin = BlockPos::new(0, 64, 0);
        let mut world = grass_world();
        let mut rng = FeatureRandom::new(4);
        assert!(place(&mut world, &mut rng, 1.0, origin));
        let podzol = world.count(states::PODZOL);
        assert!(podzol >= 5, "{podzol}");
        for (pos, state) in &world.blocks {
            if *state == states::PODZOL {
                assert_eq!(pos.y, 63);
                assert!(pos.x * pos.x + pos.z * pos.z <= 16);
            }
        }
    }

    #[test]
    fn early_returns() {
        let mut world = grass_world();
        let mut rng = FeatureRandom::new(1);
        // Occupied origin: false.
        assert!(!place(&mut world, &mut rng, 1.0, BlockPos::new(0, 63, 0)));
        // Air origin over stone: bamboo cannot survive, but Java still
        // reports true and draws nothing.
        let mut stone = TestWorld::flat(64);
        assert!(place(&mut stone, &mut rng, 1.0, BlockPos::new(0, 64, 0)));
        assert!(stone.blocks.is_empty());
        assert_eq!(rng.next_long(), FeatureRandom::new(1).next_long());
    }

    #[test]
    fn short_gap_gets_no_leaves() {
        let origin = BlockPos::new(0, 64, 0);
        let mut world = grass_world();
        world.set_state(origin.offset(0, 2, 0), states::STONE);
        let mut rng = FeatureRandom::new(1);
        assert!(place(&mut world, &mut rng, 0.0, origin));
        assert_eq!(world.get_state(origin), trunk());
        assert_eq!(world.get_state(origin.above()), trunk());
        assert_eq!(world.get_state(origin.offset(0, 2, 0)), states::STONE);
    }
}
