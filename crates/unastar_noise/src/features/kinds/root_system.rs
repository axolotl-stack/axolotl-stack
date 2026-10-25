//! `minecraft:root_system` (rooted azalea trees growing up from lush caves).

use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::RootSystemConfig;
use crate::features::eval::{FeatureWorld, provide_state, test_predicate};
use crate::features::random::FeatureRandom;
use crate::features::survival::can_survive;

/// `isAllowedTreeSpace`: air, or water within the allowed water depth
/// (`height + 1 <= allowed_vertical_water_for_tree`).
#[inline]
fn is_allowed_tree_space(
    state: crate::features::JavaState,
    height: i32,
    allowed_water: i32,
) -> bool {
    state.is_air() || (height < allowed_water && state.fluid().is_water())
}

/// `spaceForTree`: the `required_vertical_space_for_tree` blocks above `pos`.
fn space_for_tree<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &RootSystemConfig,
    pos: BlockPos,
) -> bool {
    for i in 1..=cfg.required_vertical_space_for_tree {
        let state = world.get_state(pos.offset(0, i, 0));
        if !is_allowed_tree_space(state, i, cfg.allowed_vertical_water_for_tree) {
            return false;
        }
    }
    true
}

/// `placeRootedDirt`: `root_placement_attempts` tries around the column at
/// `(x, y, z)` with offsets `nextInt(r) - nextInt(r)` on x then z.
fn place_rooted_dirt<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &RootSystemConfig,
    rng: &mut FeatureRandom,
    column: BlockPos,
) {
    let r = cfg.root_radius;
    for _ in 0..cfg.root_placement_attempts {
        let a = rng.next_int_bounded(r);
        let dx = a - rng.next_int_bounded(r);
        let c = rng.next_int_bounded(r);
        let dz = c - rng.next_int_bounded(r);
        let pos = column.offset(dx, 0, dz);
        if world.get_state(pos).is_in(cfg.root_replaceable) {
            let state = provide_state(&cfg.root_state_provider, rng, pos);
            world.set_state(pos, state);
        }
    }
}

/// `placeDirtAndTree`: climbs up to `root_column_max_height` blocks looking
/// for a position that passes `allowed_tree_position` with room above. The
/// first such position aborts the search if the block below is lava or not
/// solid; otherwise the tree is attempted there and, on success, rooted dirt
/// fills the column from the origin up to (excluding) `origin.y + i`.
fn place_dirt_and_tree<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &RootSystemConfig,
    rng: &mut FeatureRandom,
    origin: BlockPos,
    tree: &mut impl FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
) -> bool {
    let mut pos = origin;
    for i in 0..cfg.root_column_max_height {
        pos = pos.above();
        if test_predicate(world, &cfg.allowed_tree_position, pos) && space_for_tree(world, cfg, pos)
        {
            let below = pos.below();
            let below_state = world.get_state(below);
            if below_state.fluid().is_lava() || !below_state.is_solid() {
                return false;
            }
            if tree(world, rng, pos) {
                for y in origin.y..origin.y + i {
                    place_rooted_dirt(world, cfg, rng, origin.with_y(y));
                }
                return true;
            }
        }
    }
    false
}

/// `placeRoots`: `hanging_root_placement_attempts` tries at offsets
/// `nextInt(r) - nextInt(r)` (x, then y with the vertical span, then z) from
/// the origin; places a hanging root on empty blocks under a block with a
/// sturdy bottom face when the state survives there.
fn place_roots<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &RootSystemConfig,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) {
    let r = cfg.hanging_root_radius;
    let v = cfg.hanging_roots_vertical_span;
    for _ in 0..cfg.hanging_root_placement_attempts {
        let a = rng.next_int_bounded(r);
        let dx = a - rng.next_int_bounded(r);
        let c = rng.next_int_bounded(v);
        let dy = c - rng.next_int_bounded(v);
        let e = rng.next_int_bounded(r);
        let dz = e - rng.next_int_bounded(r);
        let pos = origin.offset(dx, dy, dz);
        if world.is_air(pos) {
            let state = provide_state(&cfg.hanging_root_state_provider, rng, pos);
            if can_survive(world, state, pos)
                && world.get_state(pos.above()).is_face_sturdy(Direction::Down)
            {
                world.set_state(pos, state);
            }
        }
    }
}

/// Java `minecraft:root_system` (`RootSystemFeature`).
///
/// The origin must be air. Searches upwards for the tree position (see
/// `allowed_tree_position`, `required_vertical_space_for_tree`,
/// `allowed_vertical_water_for_tree`), places the tree through `tree` (the
/// nested placed feature, which shares this random), then the rooted dirt
/// column and finally the hanging roots around the origin. Returns `true`
/// whenever the origin is air, even if no tree grew.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &RootSystemConfig,
    origin: BlockPos,
    mut tree: impl FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
) -> bool {
    if !world.is_air(origin) {
        return false;
    }
    if place_dirt_and_tree(world, cfg, rng, origin, &mut tree) {
        place_roots(world, cfg, rng, origin);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::{BlockPredicate, StateProvider};
    use crate::features::java_blocks::{blocks as b, states, tags};
    use crate::features::test_world::TestWorld;

    static ON_GRASS: [BlockPredicate; 2] = [
        BlockPredicate::Replaceable { offset: [0, 0, 0] },
        BlockPredicate::MatchingBlocks {
            set: tags::DIRT,
            offset: [0, -1, 0],
        },
    ];

    fn cfg() -> RootSystemConfig {
        RootSystemConfig {
            required_vertical_space_for_tree: 3,
            root_radius: 3,
            root_replaceable: tags::AZALEA_ROOT_REPLACEABLE,
            root_state_provider: StateProvider::Simple(states::ROOTED_DIRT),
            root_placement_attempts: 20,
            root_column_max_height: 100,
            hanging_root_radius: 3,
            hanging_roots_vertical_span: 2,
            hanging_root_state_provider: StateProvider::Simple(states::HANGING_ROOTS),
            hanging_root_placement_attempts: 20,
            allowed_vertical_water_for_tree: 2,
            allowed_tree_position: BlockPredicate::AllOf(&ON_GRASS),
        }
    }

    /// Stone up to y 19, grass at y 20, a cave from y 0 to y 4.
    fn world() -> TestWorld {
        let mut world = TestWorld::flat(20);
        for x in -6..=6 {
            for z in -6..=6 {
                for y in 0..5 {
                    world.blocks.insert(BlockPos::new(x, y, z), states::AIR);
                }
                world
                    .blocks
                    .insert(BlockPos::new(x, 20, z), states::GRASS_BLOCK);
            }
        }
        world
    }

    #[test]
    fn grows_tree_dirt_and_roots() {
        let mut world = world();
        let origin = BlockPos::new(0, 4, 0);
        let mut rng = FeatureRandom::new(4);
        let mut tree_at = None;
        let placed = place(
            &mut world,
            &mut rng,
            &cfg(),
            origin,
            |w: &mut TestWorld, _, pos| {
                tree_at = Some(pos);
                w.set_state(pos, states::AZALEA);
                true
            },
        );
        assert!(placed);
        assert_eq!(tree_at, Some(BlockPos::new(0, 21, 0)));
        let mut rooted = 0;
        let mut hanging = 0;
        for (pos, state) in world.blocks.clone() {
            if state.is(b::ROOTED_DIRT) {
                rooted += 1;
                // Column from origin.y to origin.y + i - 1 with i = 16.
                assert!(pos.y >= 4 && pos.y < 20);
                assert!(pos.x.abs() <= 2 && pos.z.abs() <= 2);
            }
            if state.is(b::HANGING_ROOTS) {
                hanging += 1;
                assert!(world.get_state(pos.above()).is_face_sturdy(Direction::Down));
            }
        }
        assert!(rooted > 0);
        assert!(hanging > 0);
    }

    #[test]
    fn failed_tree_places_nothing_but_succeeds() {
        let mut world = world();
        let before = world.blocks.len();
        let mut rng = FeatureRandom::new(4);
        let mut calls = 0;
        assert!(place(
            &mut world,
            &mut rng,
            &cfg(),
            BlockPos::new(0, 0, 0),
            |_: &mut TestWorld, _, _| {
                calls += 1;
                false
            }
        ));
        assert_eq!(world.blocks.len(), before);
        assert_eq!(calls, 1);
    }

    #[test]
    fn requires_air_origin() {
        let mut world = world();
        let mut rng = FeatureRandom::new(4);
        assert!(!place(
            &mut world,
            &mut rng,
            &cfg(),
            BlockPos::new(0, 10, 0),
            |_: &mut TestWorld, _, _| { panic!("tree must not be attempted") }
        ));
    }
}
