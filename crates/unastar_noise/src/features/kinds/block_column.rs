//! `minecraft:block_column`.

use crate::features::blocks::BlockPos;
use crate::features::config::BlockColumnConfig;
use crate::features::eval::{FeatureWorld, provide_state, test_predicate};
use crate::features::random::FeatureRandom;

/// Most layers any vanilla column uses is 3; the codec has no limit, so this
/// leaves headroom.
const MAX_LAYERS: usize = 16;

/// Java `minecraft:block_column` (`BlockColumnFeature`).
///
/// Samples every layer height in order; a total of 0 returns `false`. Then
/// walks from one block beyond the origin in `direction`, testing
/// `allowed_placement` for each of the total positions; at the first failure
/// the column is cut to the positions that passed, removing blocks from the
/// base layers first when
/// `prioritize_tip` (keeping the tip) and from the tip layers first otherwise.
/// Finally places the layers from the origin outwards, one provider draw per
/// block. Returns `true` once the total height is non-zero, even if
/// everything was cut.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &BlockColumnConfig,
    origin: BlockPos,
) -> bool {
    let layers = cfg.layers;
    assert!(
        layers.len() <= MAX_LAYERS,
        "block_column with {} layers",
        layers.len()
    );
    let mut heights = [0i32; MAX_LAYERS];
    let mut total = 0;
    for (h, layer) in heights.iter_mut().zip(layers) {
        *h = layer.height.sample(rng);
        total += *h;
    }
    if total == 0 {
        return false;
    }
    let heights = &mut heights[..layers.len()];

    // Java reserves the block beyond the last placed block as well: its
    // probe starts one step ahead of the placement cursor.
    let mut probe = origin.rel(cfg.direction);
    for placed in 0..total {
        if !test_predicate(world, &cfg.allowed_placement, probe) {
            truncate(heights, total, placed, cfg.prioritize_tip);
            break;
        }
        probe = probe.rel(cfg.direction);
    }

    let mut pos = origin;
    for (&h, layer) in heights.iter().zip(layers) {
        for _ in 0..h {
            let state = provide_state(&layer.provider, rng, pos);
            world.set_state(pos, state);
            pos = pos.rel(cfg.direction);
        }
    }
    true
}

/// Java `BlockColumnFeature.truncate`: remove `total - kept` blocks, from the
/// first layer onward when `prioritize_tip`, from the last layer backward
/// otherwise.
fn truncate(heights: &mut [i32], total: i32, kept: i32, prioritize_tip: bool) {
    let mut remove = total - kept;
    let n = heights.len();
    for i in 0..n {
        if remove <= 0 {
            break;
        }
        let idx = if prioritize_tip { i } else { n - 1 - i };
        let take = heights[idx].min(remove);
        remove -= take;
        heights[idx] -= take;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::{BlockPredicate, Direction, StateProvider};
    use crate::features::config::ColumnLayer;
    use crate::features::java_blocks::{states, tags};
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    static LAYERS: [ColumnLayer; 2] = [
        ColumnLayer {
            height: IntProvider::Constant(3),
            provider: StateProvider::Simple(states::OAK_LOG),
        },
        ColumnLayer {
            height: IntProvider::Constant(2),
            provider: StateProvider::Simple(states::OAK_LEAVES),
        },
    ];

    fn cfg(prioritize_tip: bool) -> BlockColumnConfig {
        BlockColumnConfig {
            layers: &LAYERS,
            direction: Direction::Up,
            allowed_placement: BlockPredicate::MatchingBlocks {
                set: tags::AIR,
                offset: [0, 0, 0],
            },
            prioritize_tip,
        }
    }

    fn column(world: &mut TestWorld, origin: BlockPos, n: i32) -> Vec<crate::features::JavaState> {
        (0..n)
            .map(|i| world.get_state(origin.offset(0, i, 0)))
            .collect()
    }

    #[test]
    fn full_column_when_unobstructed() {
        let mut world = TestWorld::flat(0);
        let origin = BlockPos::new(0, 0, 0);
        let mut rng = FeatureRandom::new(1);
        assert!(place(&mut world, &mut rng, &cfg(false), origin));
        let c = column(&mut world, origin, 6);
        let l = states::OAK_LOG;
        let f = states::OAK_LEAVES;
        assert_eq!(c, vec![l, l, l, f, f, states::AIR]);
    }

    #[test]
    fn truncation_respects_priority() {
        let origin = BlockPos::new(0, 0, 0);
        let l = states::OAK_LOG;
        let f = states::OAK_LEAVES;
        let s = states::STONE;
        for (tip, expected) in [
            (false, vec![l, states::AIR, s]),
            (true, vec![f, states::AIR, s]),
        ] {
            let mut world = TestWorld::flat(0);
            world.set_state(origin.offset(0, 2, 0), s);
            let mut rng = FeatureRandom::new(1);
            assert!(place(&mut world, &mut rng, &cfg(tip), origin));
            assert_eq!(
                column(&mut world, origin, 3),
                expected,
                "prioritize_tip {tip}"
            );
        }
    }

    #[test]
    fn zero_height_and_ahead_of_origin_probe() {
        static EMPTY: [ColumnLayer; 1] = [ColumnLayer {
            height: IntProvider::Constant(0),
            provider: StateProvider::Simple(states::OAK_LOG),
        }];
        let mut world = TestWorld::flat(0);
        let origin = BlockPos::new(0, 0, 0);
        let mut rng = FeatureRandom::new(1);
        let c = BlockColumnConfig {
            layers: &EMPTY,
            ..cfg(false)
        };
        assert!(!place(&mut world, &mut rng, &c, BlockPos::new(0, 0, 0)));
        // The origin itself is not probed. A one-block obstruction just above
        // it truncates to zero, but a blocked origin with air above is replaced.
        world.set_state(origin.above(), states::STONE);
        assert!(place(&mut world, &mut rng, &cfg(true), origin,));
        assert_eq!(world.count(states::OAK_LOG), 0);
        assert_eq!(world.count(states::OAK_LEAVES), 0);
        world.blocks.clear();
        assert!(place(
            &mut world,
            &mut rng,
            &cfg(true),
            BlockPos::new(0, -1, 0),
        ));
        assert_eq!(world.get_state(BlockPos::new(0, -1, 0)), states::OAK_LOG);
    }

    #[test]
    fn downward_columns_hang() {
        let mut world = TestWorld::flat(0);
        let origin = BlockPos::new(0, 10, 0);
        let mut rng = FeatureRandom::new(1);
        let c = BlockColumnConfig {
            direction: Direction::Down,
            ..cfg(false)
        };
        assert!(place(&mut world, &mut rng, &c, origin));
        assert_eq!(world.get_state(origin.offset(0, -4, 0)), states::OAK_LEAVES);
        assert_eq!(world.get_state(origin.offset(0, -5, 0)), states::AIR);
    }
}
