//! `minecraft:vines`.
//!
//! Approximation: Java's `VineBlock.isAcceptableNeighbour` accepts a
//! neighbour whose support shape or collision shape covers the touching face
//! fully; the block data only carries sturdy-face flags, so the check uses
//! [`crate::features::blocks::JavaState::is_face_sturdy`] on that face.

use crate::features::blocks::{BlockPos, Direction};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::states;
use crate::features::random::FeatureRandom;

/// Java `minecraft:vines` (`VinesFeature`).
///
/// The origin must be empty (air). Walks `Direction.values()` (down, up,
/// north, south, west, east), skipping down, and places a vine with the face
/// property of the first direction whose neighbour can carry it (`up` hangs
/// from the block above). Draws no random numbers; returns whether a vine
/// was placed.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) {
        return false;
    }
    for dir in Direction::ALL {
        if dir == Direction::Down {
            continue;
        }
        if world
            .get_state(origin.rel(dir))
            .is_face_sturdy(dir.opposite())
        {
            world.set_state(origin, states::VINE.with_bool(dir.name(), true));
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::blocks as b;
    use crate::features::test_world::TestWorld;

    #[test]
    fn hangs_from_ceiling_before_walls() {
        let mut world = TestWorld::flat(0);
        let origin = BlockPos::new(0, 10, 0);
        world.blocks.insert(origin.above(), states::STONE);
        world
            .blocks
            .insert(origin.rel(Direction::North), states::STONE);
        let mut rng = FeatureRandom::new(0);
        assert!(place(&mut world, &mut rng, origin));
        let vine = world.get_state(origin);
        assert!(vine.is(b::VINE));
        assert_eq!(vine.value("up"), Some("true"));
        assert_eq!(vine.value("north"), Some("false"));
    }

    #[test]
    fn first_acceptable_wall_in_value_order() {
        let mut world = TestWorld::flat(0);
        let origin = BlockPos::new(0, 10, 0);
        world
            .blocks
            .insert(origin.rel(Direction::East), states::STONE);
        world
            .blocks
            .insert(origin.rel(Direction::West), states::STONE);
        let mut rng = FeatureRandom::new(0);
        assert!(place(&mut world, &mut rng, origin));
        let vine = world.get_state(origin);
        assert_eq!(vine.value("west"), Some("true"));
        assert_eq!(vine.value("east"), Some("false"));
    }

    #[test]
    fn needs_air_and_support() {
        let mut world = TestWorld::flat(10);
        let mut rng = FeatureRandom::new(0);
        // Only a floor below: down is never used.
        assert!(!place(&mut world, &mut rng, BlockPos::new(0, 10, 0)));
        // Occupied origin.
        assert!(!place(&mut world, &mut rng, BlockPos::new(0, 5, 0)));
        assert!(world.blocks.is_empty());
    }
}
