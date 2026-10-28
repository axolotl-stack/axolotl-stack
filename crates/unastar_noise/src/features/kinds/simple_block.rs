//! `minecraft:simple_block`.
//!
//! Pale moss carpet side faces attach where Java's `MultifaceBlock.canAttachTo`
//! finds a full face on the neighbour; that is read from the neighbour's
//! sturdy-face flag (full-square collision face), which agrees with Java's
//! support/collision shape test for the blocks moss grows against.

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::SimpleBlockConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::FeatureRandom;
use crate::features::survival::can_survive;

/// Block types Java implements as `DoublePlantBlock` (or a subclass).
#[inline]
fn is_double_plant(state: JavaState) -> bool {
    matches!(
        state.block().def().kind,
        "minecraft:double_plant"
            | "minecraft:tall_flower"
            | "minecraft:tall_seagrass"
            | "minecraft:small_dripleaf"
            | "minecraft:pitcher_crop"
    )
}

/// Java `minecraft:simple_block` (`SimpleBlockFeature`).
///
/// Draws the state from `to_place` and returns `false` unless it can survive
/// at the origin. Double plants also need air above and are placed as lower
/// and upper halves (each copying `waterlogged` from the water at its
/// position); pale moss carpet is placed with
/// [`place_mossy_carpet`] (its side and topper draws come from the region
/// random, not the feature random); anything else is written as is. With
/// `schedule_tick`, a block tick is requested at the origin.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &SimpleBlockConfig,
    origin: BlockPos,
) -> bool {
    let state = provide_state(&cfg.to_place, rng, origin);
    if !can_survive(world, state, origin) {
        return false;
    }
    if is_double_plant(state) {
        if !world.is_air(origin.above()) {
            return false;
        }
        place_double_plant(world, state, origin);
    } else if state.is(b::PALE_MOSS_CARPET) {
        place_mossy_carpet(world, origin);
    } else {
        world.set_state(origin, state);
    }
    if cfg.schedule_tick {
        world.schedule_tick(origin);
    }
    true
}

/// Java `DoublePlantBlock.placeAt`: lower half at `pos`, upper half above,
/// each with `waterlogged` (when the block has it) set from the water there.
pub fn place_double_plant<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
) {
    let above = pos.above();
    let lower = copy_waterlogged(world, pos, state.with("half", "lower"));
    world.set_state(pos, lower);
    let upper = copy_waterlogged(world, above, state.with("half", "upper"));
    world.set_state(above, upper);
}

/// `DoublePlantBlock.copyWaterloggedFrom`.
#[inline]
fn copy_waterlogged<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    state: JavaState,
) -> JavaState {
    if state.has("waterlogged") {
        let water = world.fluid(pos).is_water();
        state.with_bool("waterlogged", water)
    } else {
        state
    }
}

/// Java `MossyCarpetBlock.placeAt(level, pos, level.getRandom(), flags)`.
///
/// Places a base pale moss carpet with low sides against every horizontal
/// neighbour that offers a full face, then builds a non-base topper above it:
/// each side the topper could have is kept on a `nextBoolean()` of the
/// region random ([`FeatureWorld::region_random`]). If the topper has any side
/// (and differs from what is already above), it is placed and the base is
/// recomputed so its sides under the topper become tall.
pub fn place_mossy_carpet<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) {
    let base = updated_carpet_state(world, states::PALE_MOSS_CARPET, pos, true);
    world.set_state(pos, base);
    let topper = carpet_topper_with_side_chance(world, pos);
    if !topper.is_air() {
        world.set_state(pos.above(), topper);
        let base = updated_carpet_state(world, base, pos, true);
        world.set_state(pos, base);
    }
}

#[inline]
fn is_base(state: JavaState) -> bool {
    state.value("bottom") == Some("true")
}

/// `MossyCarpetBlock.canSupportAtFace` for a horizontal direction.
#[inline]
fn can_support_at_face<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    dir: Direction,
) -> bool {
    world.get_state(pos.rel(dir)).is_face_sturdy(dir.opposite())
}

/// `MossyCarpetBlock.getUpdatedState`.
fn updated_carpet_state<W: FeatureWorld + ?Sized>(
    world: &mut W,
    mut state: JavaState,
    pos: BlockPos,
    create_sides: bool,
) -> JavaState {
    let mut above: Option<JavaState> = None;
    let mut below: Option<JavaState> = None;
    let create_sides = create_sides || is_base(state);
    for dir in Direction::HORIZONTAL {
        let prop = dir.name();
        let mut side = if can_support_at_face(world, pos, dir) {
            if create_sides {
                "low"
            } else {
                state.value(prop).unwrap_or("none")
            }
        } else {
            "none"
        };
        if side == "low" {
            let up = *above.get_or_insert_with(|| world.get_state(pos.above()));
            if up.is(b::PALE_MOSS_CARPET) && up.value(prop) != Some("none") && !is_base(up) {
                side = "tall";
            }
            if !is_base(state) {
                let down = *below.get_or_insert_with(|| world.get_state(pos.below()));
                if down.is(b::PALE_MOSS_CARPET) && down.value(prop) == Some("none") {
                    side = "none";
                }
            }
        }
        state = state.with(prop, side);
    }
    state
}

/// `MossyCarpetBlock.createTopperWithSideChance` with the region random's
/// `nextBoolean` as the chance.
fn carpet_topper_with_side_chance<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
) -> JavaState {
    let above_pos = pos.above();
    let above = world.get_state(above_pos);
    let is_carpet = above.is(b::PALE_MOSS_CARPET);
    // A base carpet above blocks the topper; otherwise it needs a non-base
    // carpet or a replaceable block there.
    let can_top = if is_carpet {
        !is_base(above)
    } else {
        above.can_be_replaced()
    };
    if !can_top {
        return states::AIR;
    }
    let mut topper = updated_carpet_state(
        world,
        states::PALE_MOSS_CARPET.with_bool("bottom", false),
        above_pos,
        true,
    );
    for dir in Direction::HORIZONTAL {
        let prop = dir.name();
        if topper.value(prop) != Some("none") && !world.region_random().next_bool() {
            topper = topper.with(prop, "none");
        }
    }
    let has_faces = Direction::HORIZONTAL
        .iter()
        .any(|d| topper.value(d.name()) != Some("none"));
    if has_faces && topper != above {
        topper
    } else {
        states::AIR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::test_world::TestWorld;

    fn run(
        world: &mut TestWorld,
        to_place: StateProvider,
        schedule_tick: bool,
        origin: BlockPos,
    ) -> bool {
        let cfg = SimpleBlockConfig {
            to_place,
            schedule_tick,
        };
        let mut rng = FeatureRandom::new(2);
        place(world, &mut rng, &cfg, origin)
    }

    fn grass_world() -> TestWorld {
        let mut w = TestWorld::flat(0);
        w.fill = states::GRASS_BLOCK;
        w
    }

    #[test]
    fn single_block_requires_survival() {
        let mut world = grass_world();
        let origin = BlockPos::new(0, 0, 0);
        assert!(run(
            &mut world,
            StateProvider::Simple(states::SHORT_GRASS),
            false,
            origin
        ));
        assert_eq!(world.get_state(origin), states::SHORT_GRASS);
        // On top of the grass plant there is no dirt below.
        assert!(!run(
            &mut world,
            StateProvider::Simple(states::SHORT_GRASS),
            false,
            origin.above()
        ));
        assert!(world.ticks.is_empty());
    }

    #[test]
    fn double_plant_places_both_halves_and_needs_room() {
        let mut world = grass_world();
        let origin = BlockPos::new(0, 0, 0);
        let tall = states::TALL_GRASS.with("half", "lower");
        assert!(run(&mut world, StateProvider::Simple(tall), true, origin));
        assert_eq!(world.get_state(origin), tall);
        assert_eq!(world.get_state(origin.above()), tall.with("half", "upper"));
        assert_eq!(world.ticks, vec![origin]);

        let blocked = BlockPos::new(5, 0, 5);
        world.set_state(blocked.above(), states::STONE);
        assert!(!run(
            &mut world,
            StateProvider::Simple(tall),
            false,
            blocked
        ));
        assert!(world.get_state(blocked).is_air());
    }

    #[test]
    fn pale_moss_carpet_grows_sides_against_walls() {
        let origin = BlockPos::new(0, 0, 0);
        let mut world = grass_world();
        // A two-high wall to the north, nothing elsewhere.
        world.set_state(origin.offset(0, 0, -1), states::STONE);
        world.set_state(origin.offset(0, 1, -1), states::STONE);
        let carpet = states::PALE_MOSS_CARPET;
        assert!(run(
            &mut world,
            StateProvider::Simple(carpet),
            false,
            origin
        ));
        let base = world.get_state(origin);
        assert!(base.is(b::PALE_MOSS_CARPET) && is_base(base));
        for dir in [Direction::East, Direction::South, Direction::West] {
            assert_eq!(base.value(dir.name()), Some("none"));
        }
        let top = world.get_state(origin.above());
        if top.is(b::PALE_MOSS_CARPET) {
            // The topper kept its north side: the base side becomes tall.
            assert!(!is_base(top));
            assert_eq!(top.value("north"), Some("low"));
            assert_eq!(base.value("north"), Some("tall"));
        } else {
            assert!(top.is_air());
            assert_eq!(base.value("north"), Some("low"));
        }
    }

    #[test]
    fn pale_moss_carpet_does_not_use_feature_random() {
        let origin = BlockPos::new(0, 0, 0);
        let mut world = grass_world();
        world.set_state(origin.offset(1, 0, 0), states::STONE);
        world.set_state(origin.offset(1, 1, 0), states::STONE);
        let cfg = SimpleBlockConfig {
            to_place: StateProvider::Simple(states::PALE_MOSS_CARPET),
            schedule_tick: false,
        };
        let mut rng = FeatureRandom::new(9);
        assert!(place(&mut world, &mut rng, &cfg, origin));
        assert_eq!(rng.next_long(), FeatureRandom::new(9).next_long());
    }
}
