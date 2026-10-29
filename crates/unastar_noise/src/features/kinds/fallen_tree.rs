//! `minecraft:fallen_tree` (Java `FallenTreeFeature`, 1.21.5+).
//!
//! A one-block stump at the origin, then a log lying 2-3 blocks away in a
//! random horizontal direction, dropped onto the ground (up to 5 blocks
//! down) and placed only if every block is a valid tree position with gaps
//! in its ground support of at most 2. Stump and log run their decorators
//! through the shared tree decorator code with empty leaf and root lists.
//!
//! Java marks the blocks above each placed log for post-processing; when the
//! chunk is finalised that re-runs the block's shape update, which removes
//! the upper half of a double plant whose lower half the log replaced. The
//! runtime has no post-processing queue, so the same shape update is applied
//! right away to the block above every log (see
//! [`crate::features::shape`] for the native rules).

use super::tree::{JavaPosSet, run_decorators, valid_tree_pos};
use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::FallenTreeConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::random::FeatureRandom;
use crate::features::shape::update_from_neighbour_shapes;

/// Place a `minecraft:fallen_tree` feature. Always returns `true`, like Java.
///
/// `pale_moss_patch` is forwarded to the decorators (only the `pale_moss`
/// decorator calls it).
pub fn place<W, F>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &FallenTreeConfig,
    origin: BlockPos,
    mut pale_moss_patch: F,
) -> bool
where
    W: FeatureWorld + ?Sized,
    F: FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
{
    // Stump.
    place_log_block(world, rng, cfg, origin, None);
    if !cfg.stump_decorators.is_empty() {
        run_decorators(
            world,
            rng,
            cfg.stump_decorators,
            vec![origin],
            Vec::new(),
            Vec::new(),
            None,
            &mut pale_moss_patch,
        );
    }

    let dir = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
    let log_length = cfg.log_length.sample(rng) - 2;
    let mut start = origin.rel_n(dir, 2 + rng.next_int_bounded(2));
    // setGroundHeightForFallenLogStartPos: from one above, scan down 6.
    start = start.above();
    for _ in 0..6 {
        if may_place_on(world, start) {
            break;
        }
        start = start.below();
    }
    if !can_place_entire_log(world, log_length, start, dir) {
        return true;
    }
    let mut logs = JavaPosSet::new();
    let mut pos = start;
    for _ in 0..log_length {
        place_log_block(world, rng, cfg, pos, Some(dir.axis()));
        logs.add(pos);
        pos = pos.rel(dir);
    }
    if !cfg.log_decorators.is_empty() {
        run_decorators(
            world,
            rng,
            cfg.log_decorators,
            logs.to_vec(),
            Vec::new(),
            Vec::new(),
            None,
            &mut pale_moss_patch,
        );
    }
    true
}

/// `isOverSolidGround`: the block below has a sturdy top face.
#[inline]
fn over_solid_ground<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    world.get_state(pos.below()).is_face_sturdy(Direction::Up)
}

#[inline]
fn may_place_on<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    valid_tree_pos(world, pos) && over_solid_ground(world, pos)
}

/// `canPlaceEntireFallenLog`.
fn can_place_entire_log<W: FeatureWorld + ?Sized>(
    world: &mut W,
    length: i32,
    start: BlockPos,
    dir: Direction,
) -> bool {
    let mut gap = 0;
    let mut pos = start;
    for _ in 0..length {
        if !valid_tree_pos(world, pos) {
            return false;
        }
        if !over_solid_ground(world, pos) {
            gap += 1;
            if gap > 2 {
                return false;
            }
        } else {
            gap = 0;
        }
        pos = pos.rel(dir);
    }
    true
}

/// `placeLogBlock`: trunk provider state (sideways logs get the direction's
/// axis), then the post-processing shape update of the block above.
fn place_log_block<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &FallenTreeConfig,
    pos: BlockPos,
    axis: Option<&'static str>,
) {
    let mut state = provide_state(&cfg.trunk_provider, rng, pos);
    if let Some(axis) = axis {
        state = state.try_with("axis", axis);
    }
    world.set_state(pos, state);
    post_process(world, pos.above());
}

/// `Block.updateFromNeighbourShapes` at `pos` (what Java's post-processing
/// does to the positions a fallen log marks).
fn post_process<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) {
    let original = world.get_state(pos);
    let state = update_from_neighbour_shapes(world, pos, original);
    if state != original {
        world.set_state(pos, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::config::TreeDecorator;
    use crate::features::java_blocks::{blocks as b, states, tags};
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    const SURFACE: i32 = 64;

    fn no_patch(_: &mut TestWorld, _: &mut FeatureRandom, _: BlockPos) -> bool {
        false
    }

    fn cfg(decorators: &'static [TreeDecorator]) -> FallenTreeConfig {
        FallenTreeConfig {
            trunk_provider: StateProvider::Simple(states::OAK_LOG),
            log_length: IntProvider::Uniform { min: 4, max: 7 },
            stump_decorators: &[],
            log_decorators: decorators,
        }
    }

    fn logs(w: &TestWorld) -> Vec<(BlockPos, crate::features::blocks::JavaState)> {
        let mut v: Vec<_> = w
            .blocks
            .iter()
            .filter(|(_, s)| s.is(b::OAK_LOG))
            .map(|(p, s)| (*p, *s))
            .collect();
        v.sort_by_key(|(p, _)| (p.x, p.y, p.z));
        v
    }

    #[test]
    fn log_lies_on_the_ground_beside_the_stump() {
        for seed in 0..30 {
            let mut w = TestWorld::flat(SURFACE);
            let mut rng = FeatureRandom::new(seed);
            assert!(place(
                &mut w,
                &mut rng,
                &cfg(&[]),
                BlockPos::new(0, SURFACE, 0),
                no_patch
            ));
            let all = logs(&w);
            let stump = BlockPos::new(0, SURFACE, 0);
            assert_eq!(w.blocks[&stump], states::OAK_LOG);
            let fallen: Vec<_> = all.iter().filter(|(p, _)| *p != stump).collect();
            // log_length 4..=7 minus 2.
            assert!(
                (2..=5).contains(&fallen.len()),
                "seed {seed}: {}",
                fallen.len()
            );
            let axis = fallen[0].1.value("axis").unwrap();
            assert_ne!(axis, "y");
            for (p, s) in &fallen {
                assert_eq!(p.y, SURFACE);
                assert_eq!(s.value("axis"), Some(axis));
                // On a straight line off the stump, 2..=3 + length away.
                let d = p.x.abs() + p.z.abs();
                assert!(
                    (2..=8).contains(&d) && (p.x == 0 || p.z == 0),
                    "seed {seed}: {p:?}"
                );
            }
        }
    }

    #[test]
    fn log_is_skipped_over_a_wide_gap() {
        // A deep trench around the stump: the log never finds ground, and a
        // log of 3+ blocks cannot bridge more than two unsupported blocks.
        for seed in 0..10 {
            let mut w = TestWorld::flat(SURFACE);
            for x in -12..=12 {
                for z in -12..=12 {
                    if x != 0 || z != 0 {
                        for y in SURFACE - 8..SURFACE {
                            w.blocks.insert(BlockPos::new(x, y, z), states::AIR);
                        }
                    }
                }
            }
            let mut rng = FeatureRandom::new(seed);
            let mut c = cfg(&[]);
            c.log_length = IntProvider::Uniform { min: 5, max: 7 };
            assert!(place(
                &mut w,
                &mut rng,
                &c,
                BlockPos::new(0, SURFACE, 0),
                no_patch
            ));
            assert_eq!(logs(&w).len(), 1, "seed {seed}");
        }
    }

    #[test]
    fn deterministic_and_decorators_attach_to_logs_only() {
        static DECORATORS: [TreeDecorator; 2] = [
            TreeDecorator::AttachedToLogs {
                probability: 0.5,
                block_provider: StateProvider::Simple(states::RED_MUSHROOM),
                directions: &[Direction::Up],
            },
            TreeDecorator::TrunkVine,
        ];
        for seed in 0..20 {
            let run = || {
                let mut w = TestWorld::flat(SURFACE);
                let mut rng = FeatureRandom::new(seed);
                place(
                    &mut w,
                    &mut rng,
                    &cfg(&DECORATORS),
                    BlockPos::new(0, SURFACE, 0),
                    no_patch,
                );
                w
            };
            let w = run();
            assert_eq!(w.blocks, run().blocks);
            for (p, s) in &w.blocks {
                if s.is(b::RED_MUSHROOM) {
                    assert!(w.blocks[&p.below()].is_in(tags::LOGS), "mushroom at {p:?}");
                }
            }
        }
    }

    #[test]
    fn upper_half_above_replaced_lower_half_is_removed() {
        let mut w = TestWorld::flat(SURFACE);
        let origin = BlockPos::new(0, SURFACE, 0);
        w.blocks
            .insert(origin, b::TALL_GRASS.default_state().with("half", "lower"));
        w.blocks.insert(
            origin.above(),
            b::TALL_GRASS.default_state().with("half", "upper"),
        );
        let mut rng = FeatureRandom::new(4);
        place(&mut w, &mut rng, &cfg(&[]), origin, no_patch);
        assert_eq!(w.blocks[&origin], states::OAK_LOG);
        assert_eq!(w.blocks[&origin.above()], states::AIR);
    }
}
