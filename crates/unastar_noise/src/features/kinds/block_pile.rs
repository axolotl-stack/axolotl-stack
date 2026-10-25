//! `minecraft:block_pile`.

use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::BlockPileConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::blocks as b;
use crate::features::random::FeatureRandom;

/// Java `minecraft:block_pile` (`BlockPileFeature`).
///
/// Returns `false` below `min_y + 5`. Otherwise draws the x and z half-sizes
/// (`2 + nextInt(2)` each) and walks the box `[-rx, rx] x [0, 1] x [-rz, rz]`
/// (x fastest, z outermost). A position inside the jittered ellipse
/// `dx² + dz² <= nextFloat() * 10 - nextFloat() * 6`, or else on a
/// `nextFloat() < 0.031` chance, gets one placement attempt: it must be air
/// and stand on a sturdy top face (on dirt paths: a `nextBoolean()` instead),
/// then takes the provider's state. Always `true` otherwise.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &BlockPileConfig,
    origin: BlockPos,
) -> bool {
    if origin.y < world.min_y() + 5 {
        return false;
    }
    let rx = 2 + rng.next_int_bounded(2);
    let rz = 2 + rng.next_int_bounded(2);
    for z in origin.z - rz..=origin.z + rz {
        for y in origin.y..=origin.y + 1 {
            for x in origin.x - rx..=origin.x + rx {
                let pos = BlockPos::new(x, y, z);
                let dx = origin.x - x;
                let dz = origin.z - z;
                let a = rng.next_float();
                let b = rng.next_float();
                // The 0.031 chance is drawn only outside the ellipse.
                let inside = (dx * dx + dz * dz) as f32 <= a * 10.0 - b * 6.0;
                if inside || (rng.next_float() as f64) < 0.031 {
                    try_place(world, rng, cfg, pos);
                }
            }
        }
    }
    true
}

/// `BlockPileFeature.tryPlaceBlock`.
fn try_place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &BlockPileConfig,
    pos: BlockPos,
) {
    if world.is_air(pos) && may_place_on(world, rng, pos) {
        let state = provide_state(&cfg.state_provider, rng, pos);
        world.set_state(pos, state);
    }
}

/// `BlockPileFeature.mayPlaceOn`.
fn may_place_on<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    pos: BlockPos,
) -> bool {
    let below = world.get_state(pos.below());
    if below.is(b::DIRT_PATH) {
        rng.next_bool()
    } else {
        below.is_face_sturdy(Direction::Up)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;

    const HAY: BlockPileConfig = BlockPileConfig {
        state_provider: StateProvider::Rotated(states::HAY_BLOCK),
    };

    #[test]
    fn pile_stays_in_box_and_on_ground() {
        for seed in 0..32 {
            let mut world = TestWorld::flat(64);
            let origin = BlockPos::new(0, 64, 0);
            let mut rng = FeatureRandom::new(seed);
            assert!(place(&mut world, &mut rng, &HAY, origin));
            assert!(!world.blocks.is_empty(), "seed {seed}");
            for (pos, state) in &world.blocks {
                assert!(state.is(b::HAY_BLOCK));
                assert!(pos.x.abs() <= 3 && pos.z.abs() <= 3);
                assert!(pos.y == 64 || pos.y == 65, "{pos:?}");
                if pos.y == 65 {
                    assert!(
                        world.blocks.contains_key(&pos.below()),
                        "floating at {pos:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn deterministic_and_low_bail() {
        let origin = BlockPos::new(4, 64, -4);
        let mut a = TestWorld::flat(64);
        let mut b = TestWorld::flat(64);
        place(&mut a, &mut FeatureRandom::new(5), &HAY, origin);
        place(&mut b, &mut FeatureRandom::new(5), &HAY, origin);
        assert_eq!(a.blocks, b.blocks);

        let mut low = TestWorld::flat(-64);
        assert!(!place(
            &mut low,
            &mut FeatureRandom::new(5),
            &HAY,
            BlockPos::new(0, -60, 0)
        ));
        assert!(low.blocks.is_empty());
    }
}
