//! `minecraft:disk`.
//!
//! Java also marks the two blocks above the topmost replaced block of each run
//! for chunk post-processing (neighbour-shape updates applied when the chunk
//! is finalised). That is chunk bookkeeping, not a block write or a random
//! draw, and is left to the runtime's own post-processing pass.

use crate::features::blocks::BlockPos;
use crate::features::config::DiskConfig;
use crate::features::eval::{FeatureWorld, provide_rule_based, test_predicate};
use crate::features::random::FeatureRandom;

/// Java `minecraft:disk` (`DiskFeature`).
///
/// Samples the radius, then walks the square `[-r, r]` (x fastest, z
/// outermost, as `BlockPos.betweenClosed`) and, for every column within the
/// circle `dx² + dz² <= r²`, scans from `y + half_height` down to
/// `y - half_height` replacing every block that matches `target` with the
/// rule-based provider's state. Returns whether any block was replaced.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &DiskConfig,
    origin: BlockPos,
) -> bool {
    let top = origin.y + cfg.half_height;
    let bottom = origin.y - cfg.half_height - 1;
    let r = cfg.radius.sample(rng);
    let mut placed = false;
    for z in origin.z - r..=origin.z + r {
        for x in origin.x - r..=origin.x + r {
            let dx = x - origin.x;
            let dz = z - origin.z;
            if dx * dx + dz * dz <= r * r {
                placed |= place_column(world, rng, cfg, top, bottom, x, z);
            }
        }
    }
    placed
}

/// Java `DiskFeature.placeColumn`: top-down over `(bottom, top]`.
fn place_column<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &DiskConfig,
    top: i32,
    bottom: i32,
    x: i32,
    z: i32,
) -> bool {
    let mut placed = false;
    let mut y = top;
    while y > bottom {
        let pos = BlockPos::new(x, y, z);
        if test_predicate(world, &cfg.target, pos) {
            let state = provide_rule_based(world, &cfg.state_provider, rng, pos);
            world.set_state(pos, state);
            placed = true;
        }
        y -= 1;
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::{BlockPredicate, RuleBasedStateProvider, StateProvider};
    use crate::features::java_blocks::{states, tags};
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    const ABOVE_SOLID: BlockPredicate = BlockPredicate::Solid { offset: [0, 1, 0] };
    static RULES: [(BlockPredicate, StateProvider); 1] =
        [(ABOVE_SOLID, StateProvider::Simple(states::GRAVEL))];

    fn cfg(radius: IntProvider) -> DiskConfig {
        DiskConfig {
            state_provider: RuleBasedStateProvider {
                fallback: StateProvider::Simple(states::CLAY),
                rules: &RULES,
            },
            target: BlockPredicate::MatchingBlocks {
                set: tags::DIRT,
                offset: [0, 0, 0],
            },
            radius,
            half_height: 1,
        }
    }

    fn dirt_world() -> TestWorld {
        let mut w = TestWorld::flat(0);
        w.fill = states::DIRT;
        w
    }

    #[test]
    fn replaces_only_targets_in_circle_and_slab() {
        let mut world = dirt_world();
        let origin = BlockPos::new(0, -1, 0);
        let mut rng = FeatureRandom::new(3);
        assert!(place(
            &mut world,
            &mut rng,
            &cfg(IntProvider::Constant(3)),
            origin
        ));
        for (pos, state) in &world.blocks {
            assert!(pos.x * pos.x + pos.z * pos.z <= 9, "{pos:?}");
            assert!((-2..=-1).contains(&pos.y), "{pos:?}");
            // The block under the topmost dirt has solid above it -> gravel;
            // the top layer has air above -> clay fallback.
            let expected = if pos.y == -1 {
                states::CLAY
            } else {
                states::GRAVEL
            };
            assert_eq!(*state, expected, "{pos:?}");
        }
        // 29 columns in a radius-3 disk, two dirt layers each.
        assert_eq!(world.blocks.len(), 29 * 2);
    }

    #[test]
    fn no_target_returns_false() {
        let mut world = TestWorld::flat(0);
        let mut rng = FeatureRandom::new(3);
        assert!(!place(
            &mut world,
            &mut rng,
            &cfg(IntProvider::Constant(2)),
            BlockPos::new(0, -1, 0)
        ));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn radius_sampled_once_and_deterministic() {
        let c = cfg(IntProvider::Uniform { min: 2, max: 6 });
        let mut a = dirt_world();
        let mut b = dirt_world();
        let mut ra = FeatureRandom::new(11);
        let mut rb = FeatureRandom::new(11);
        place(&mut a, &mut ra, &c, BlockPos::new(4, -1, 4));
        place(&mut b, &mut rb, &c, BlockPos::new(4, -1, 4));
        assert_eq!(a.blocks, b.blocks);
        // Simple providers draw nothing: only the radius was consumed.
        let mut reference = FeatureRandom::new(11);
        c.radius.sample(&mut reference);
        assert_eq!(ra.next_long(), reference.next_long());
    }
}
