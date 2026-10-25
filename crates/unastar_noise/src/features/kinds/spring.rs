//! `minecraft:spring_feature`.

use crate::features::blocks::BlockPos;
use crate::features::config::SpringConfig;
use crate::features::eval::FeatureWorld;
use crate::features::random::FeatureRandom;

/// Java `minecraft:spring_feature` (`SpringFeature`).
///
/// Requires a valid block above (and below when `requires_block_below`) and
/// air or a valid block at the origin. Counts valid blocks among west, east,
/// north, south and below, then air among the same five; only when both
/// counts equal `rock_count` and `hole_count` exactly does it place
/// `cfg.state` (the fluid's legacy block state, e.g. a water source) and
/// schedule a fluid tick. Draws no random numbers; returns whether the fluid
/// was placed.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    cfg: &SpringConfig,
    origin: BlockPos,
) -> bool {
    let valid = cfg.valid_blocks;
    if !world.get_state(origin.above()).is_in(valid) {
        return false;
    }
    if cfg.requires_block_below && !world.get_state(origin.below()).is_in(valid) {
        return false;
    }
    let at = world.get_state(origin);
    if !at.is_air() && !at.is_in(valid) {
        return false;
    }
    let sides = [
        origin.offset(-1, 0, 0),
        origin.offset(1, 0, 0),
        origin.offset(0, 0, -1),
        origin.offset(0, 0, 1),
        origin.below(),
    ];
    let mut rocks = 0;
    for pos in sides {
        if world.get_state(pos).is_in(valid) {
            rocks += 1;
        }
    }
    let mut holes = 0;
    for pos in sides {
        if world.is_air(pos) {
            holes += 1;
        }
    }
    if rocks == cfg.rock_count && holes == cfg.hole_count {
        world.set_state(origin, cfg.state);
        world.schedule_tick(origin);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::{states, tags};
    use crate::features::test_world::TestWorld;

    const WATER: SpringConfig = SpringConfig {
        state: states::WATER,
        requires_block_below: true,
        rock_count: 4,
        hole_count: 1,
        valid_blocks: tags::BASE_STONE_OVERWORLD,
    };

    fn run(world: &mut TestWorld, cfg: &SpringConfig, origin: BlockPos) -> bool {
        let mut rng = FeatureRandom::new(0);
        place(world, &mut rng, cfg, origin)
    }

    #[test]
    fn places_in_wall_with_one_opening() {
        let mut world = TestWorld::flat(64);
        let origin = BlockPos::new(0, 10, 0);
        world.set_state(origin, states::AIR);
        world.set_state(origin.offset(1, 0, 0), states::AIR);
        assert!(run(&mut world, &WATER, origin));
        assert_eq!(world.get_state(origin), states::WATER);
        assert_eq!(world.ticks, vec![origin]);
    }

    #[test]
    fn exact_counts_required() {
        // Fully enclosed: 5 rocks, 0 holes.
        let mut world = TestWorld::flat(64);
        let origin = BlockPos::new(0, 10, 0);
        assert!(!run(&mut world, &WATER, origin));
        // Two openings: 3 rocks, 2 holes.
        world.set_state(origin.offset(1, 0, 0), states::AIR);
        world.set_state(origin.offset(-1, 0, 0), states::AIR);
        assert!(!run(&mut world, &WATER, origin));
        assert!(world.ticks.is_empty());
    }

    #[test]
    fn needs_valid_above_and_below() {
        let mut world = TestWorld::flat(64);
        let origin = BlockPos::new(0, 10, 0);
        world.set_state(origin.offset(1, 0, 0), states::AIR);
        world.set_state(origin.below(), states::DIRT);
        assert!(!run(&mut world, &WATER, origin));
        let lenient = SpringConfig {
            requires_block_below: false,
            rock_count: 3,
            hole_count: 1,
            ..WATER
        };
        assert!(run(&mut world, &lenient, origin));
        world.set_state(origin.above(), states::AIR);
        world.set_state(origin, states::STONE);
        assert!(!run(&mut world, &lenient, origin));
    }
}
