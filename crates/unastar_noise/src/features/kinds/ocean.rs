//! Ocean floor features: `minecraft:kelp`, `minecraft:seagrass`,
//! `minecraft:sea_pickle` and the coral reefs `minecraft:coral_claw`,
//! `minecraft:coral_mushroom`, `minecraft:coral_tree`.
//!
//! Random tag members (`Registry.getRandomElementOf(tag, random)`) are drawn
//! with `nextInt(size)` over the tag's resolved entry order, which for the
//! coral tags is the order of the vanilla tag files (`coral_blocks`, `corals`
//! = `#coral_plants` then the fans, `wall_corals`). The block lists below
//! mirror those tags; a unit test checks them against the generated tag sets.

use crate::features::blocks::{BlockPos, Direction, Heightmap, JavaState};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::kinds::multiface_growth::java_shuffle;
use crate::features::random::FeatureRandom;
use crate::features::survival::can_survive;
use crate::features::values::IntProvider;

/// `#minecraft:coral_blocks` in tag order.
const CORAL_BLOCKS: [JavaState; 5] = [
    states::TUBE_CORAL_BLOCK,
    states::BRAIN_CORAL_BLOCK,
    states::BUBBLE_CORAL_BLOCK,
    states::FIRE_CORAL_BLOCK,
    states::HORN_CORAL_BLOCK,
];

/// `#minecraft:corals` in tag order (`#coral_plants`, then the fans).
const CORALS: [JavaState; 10] = [
    states::TUBE_CORAL,
    states::BRAIN_CORAL,
    states::BUBBLE_CORAL,
    states::FIRE_CORAL,
    states::HORN_CORAL,
    states::TUBE_CORAL_FAN,
    states::BRAIN_CORAL_FAN,
    states::BUBBLE_CORAL_FAN,
    states::FIRE_CORAL_FAN,
    states::HORN_CORAL_FAN,
];

/// `#minecraft:wall_corals` in tag order.
const WALL_CORALS: [JavaState; 5] = [
    states::TUBE_CORAL_WALL_FAN,
    states::BRAIN_CORAL_WALL_FAN,
    states::BUBBLE_CORAL_WALL_FAN,
    states::FIRE_CORAL_WALL_FAN,
    states::HORN_CORAL_WALL_FAN,
];

/// `Util.getRandomSafe(list, random)` on a non-empty list.
#[inline]
fn pick<T: Copy>(list: &[T], rng: &mut FeatureRandom) -> T {
    list[rng.next_int_bounded(list.len() as i32) as usize]
}

/// `nextInt(8) - nextInt(8)` horizontal scatter used by seagrass and pickles.
#[inline]
fn scatter(rng: &mut FeatureRandom) -> i32 {
    let a = rng.next_int_bounded(8);
    a - rng.next_int_bounded(8)
}

/// Java `minecraft:kelp` (`KelpFeature`).
///
/// Starts at the `OCEAN_FLOOR` height of the origin column (Java reads the
/// `OCEAN_FLOOR` heightmap here, which during decoration tracks the same
/// motion-blocking surface as `OCEAN_FLOOR_WG`). If that block is water,
/// draws a height `1 + nextInt(10)` and grows kelp plant blocks while the
/// block and the one above are water and the plant survives; the top block is
/// a kelp head with `age = nextInt(4) + 20` (20..=23). If the column is cut
/// short, the block below the stopping point becomes the head instead (when
/// it can survive and is not stacked on another head). Returns whether a
/// head was placed.
pub fn place_kelp<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let mut placed = 0;
    let y = world.get_height(Heightmap::OceanFloor, origin.x, origin.z);
    let mut pos = origin.with_y(y);
    if !world.get_state(pos).is(b::WATER) {
        return false;
    }
    let kelp = states::KELP;
    let kelp_plant = states::KELP_PLANT;
    let height = 1 + rng.next_int_bounded(10);
    for h in 0..=height {
        if world.get_state(pos).is(b::WATER)
            && world.get_state(pos.above()).is(b::WATER)
            && can_survive(world, kelp_plant, pos)
        {
            if h == height {
                let age = rng.next_int_bounded(4) + 20;
                world.set_state(pos, kelp.with_int("age", age));
                placed += 1;
            } else {
                world.set_state(pos, kelp_plant);
            }
        } else if h > 0 {
            let below = pos.below();
            if can_survive(world, kelp, below) && !world.get_state(below.below()).is(b::KELP) {
                let age = rng.next_int_bounded(4) + 20;
                world.set_state(below, kelp.with_int("age", age));
                placed += 1;
            }
            break;
        }
        pos = pos.above();
    }
    placed > 0
}

/// Java `minecraft:seagrass` (`SeagrassFeature`, `probability` config).
///
/// Picks a column at `nextInt(8) - nextInt(8)` on x then z, takes its
/// `OCEAN_FLOOR` height and, if that block is water, chooses tall seagrass
/// when `nextDouble() < probability`. Tall seagrass also needs water above
/// for its upper half; if that is missing nothing is placed but the feature
/// still reports success (as in vanilla, success only depends on survival).
pub fn place_seagrass<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    probability: f32,
    origin: BlockPos,
) -> bool {
    let dx = scatter(rng);
    let dz = scatter(rng);
    let x = origin.x + dx;
    let z = origin.z + dz;
    let y = world.get_height(Heightmap::OceanFloor, x, z);
    let pos = BlockPos::new(x, y, z);
    if !world.get_state(pos).is(b::WATER) {
        return false;
    }
    let tall = rng.next_double() < probability as f64;
    let state = if tall {
        states::TALL_SEAGRASS
    } else {
        states::SEAGRASS
    };
    if !can_survive(world, state, pos) {
        return false;
    }
    if tall {
        let upper = state.with("half", "upper");
        let above = pos.above();
        if world.get_state(above).is(b::WATER) {
            world.set_state(pos, state);
            world.set_state(above, upper);
        }
    } else {
        world.set_state(pos, state);
    }
    true
}

/// Java `minecraft:sea_pickle` (`SeaPickleFeature`, `count` config).
///
/// Samples `count`, then per attempt scatters x and z by
/// `nextInt(8) - nextInt(8)`, draws `pickles = nextInt(4) + 1` and places the
/// pickle at the `OCEAN_FLOOR` height when that block is water and the pickle
/// survives. Returns whether any pickle was placed.
pub fn place_sea_pickle<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    count: &IntProvider,
    origin: BlockPos,
) -> bool {
    let mut placed = 0;
    let n = count.sample(rng);
    for _ in 0..n {
        let dx = scatter(rng);
        let dz = scatter(rng);
        let x = origin.x + dx;
        let z = origin.z + dz;
        let y = world.get_height(Heightmap::OceanFloor, x, z);
        let pos = BlockPos::new(x, y, z);
        let state = states::SEA_PICKLE.with_int("pickles", rng.next_int_bounded(4) + 1);
        if world.get_state(pos).is(b::WATER) && can_survive(world, state, pos) {
            world.set_state(pos, state);
            placed += 1;
        }
    }
    placed > 0
}

/// `CoralFeature.placeCoralBlock`: puts `block` at `pos` if it is water or a
/// coral (`#corals`) with water above. Then: with 0.25 a random coral or fan
/// on top, else with 0.05 a sea pickle (`nextInt(4) + 1` pickles); then per
/// horizontal side (N, E, S, W) with 0.2 a random wall fan facing that side
/// if the side is water.
fn place_coral_block<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    pos: BlockPos,
    block: JavaState,
) -> bool {
    let above = pos.above();
    let current = world.get_state(pos);
    if !((current.is(b::WATER) || current.is_in(tags::CORALS))
        && world.get_state(above).is(b::WATER))
    {
        return false;
    }
    world.set_state(pos, block);
    if rng.next_float() < 0.25 {
        let coral = pick(&CORALS, rng);
        world.set_state(above, coral);
    } else if rng.next_float() < 0.05 {
        let pickles = rng.next_int_bounded(4) + 1;
        world.set_state(above, states::SEA_PICKLE.with_int("pickles", pickles));
    }
    for dir in Direction::HORIZONTAL {
        if rng.next_float() < 0.2 {
            let side = pos.rel(dir);
            if world.get_state(side).is(b::WATER) {
                let fan = pick(&WALL_CORALS, rng).try_with("facing", dir.name());
                world.set_state(side, fan);
            }
        }
    }
    true
}

/// `CoralFeature.place`: a random `#coral_blocks` block, then the shape.
fn coral_block(rng: &mut FeatureRandom) -> JavaState {
    pick(&CORAL_BLOCKS, rng)
}

/// Java `minecraft:coral_claw` (`CoralClawFeature`).
///
/// Places the base coral block, then picks a main direction (`nextInt(4)`
/// over N, E, S, W), an arm count `nextInt(2) + 2`, and shuffles
/// `[main, clockwise, counter-clockwise]`. Each arm grows a short side run
/// (`nextInt(2) + 1`; the main arm continues straight with length
/// `nextInt(3) + 2`, side arms step up and pick their run direction from
/// `[side, up]` with length `nextInt(3) + 3`), then extends towards the main
/// direction, stepping up with 0.25 per block. Returns `false` only when the
/// base block cannot be placed.
pub fn place_coral_claw<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let block = coral_block(rng);
    if !place_coral_block(world, rng, origin, block) {
        return false;
    }
    let main = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
    let arms = (rng.next_int_bounded(2) + 2) as usize;
    let mut dirs = [main, main.clockwise(), main.counter_clockwise()];
    java_shuffle(&mut dirs, rng);
    for &dir in &dirs[..arms] {
        let mut p = origin;
        let side_len = rng.next_int_bounded(2) + 1;
        p = p.rel(dir);
        let (arm_dir, arm_len) = if dir == main {
            (main, rng.next_int_bounded(3) + 2)
        } else {
            p = p.above();
            let choices = [dir, Direction::Up];
            let arm_dir = choices[rng.next_int_bounded(2) as usize];
            (arm_dir, rng.next_int_bounded(3) + 3)
        };
        let mut i = 0;
        while i < side_len && place_coral_block(world, rng, p, block) {
            p = p.rel(arm_dir);
            i += 1;
        }
        p = p.rel(arm_dir.opposite());
        p = p.above();
        for _ in 0..arm_len {
            p = p.rel(main);
            if !place_coral_block(world, rng, p, block) {
                break;
            }
            if rng.next_float() < 0.25 {
                p = p.above();
            }
        }
    }
    true
}

/// Java `minecraft:coral_mushroom` (`CoralMushroomFeature`).
///
/// Draws height `nextInt(3) + 3`, x size `nextInt(3) + 3`, z size
/// `nextInt(3) + 3` and a sink depth `nextInt(3) + 1`, then walks the box
/// (x outer, y, z inner) shifted down by the sink depth and places coral on
/// its surface without the box edges, skipping each block with 0.1. Always
/// returns `true`.
pub fn place_coral_mushroom<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let block = coral_block(rng);
    let height = rng.next_int_bounded(3) + 3;
    let size_x = rng.next_int_bounded(3) + 3;
    let size_z = rng.next_int_bounded(3) + 3;
    let sink = rng.next_int_bounded(3) + 1;
    for x in 0..=size_x {
        for y in 0..=height {
            for z in 0..=size_z {
                let pos = origin.offset(x, y - sink, z);
                let x_inner = x != 0 && x != size_x;
                let y_inner = y != 0 && y != height;
                let z_inner = z != 0 && z != size_z;
                let not_edge = (x_inner || y_inner) && (z_inner || y_inner) && (x_inner || z_inner);
                let on_surface = !x_inner || !y_inner || !z_inner;
                if not_edge && on_surface && !(rng.next_float() < 0.1) {
                    place_coral_block(world, rng, pos, block);
                }
            }
        }
    }
    true
}

/// Java `minecraft:coral_tree` (`CoralTreeFeature`).
///
/// Grows a trunk of `nextInt(3) + 1` coral blocks (stopping early, still
/// returning `true`, when one cannot be placed), then `nextInt(3) + 2`
/// branches in a shuffled copy of the horizontal directions. Each branch
/// starts beside the trunk top and climbs up to `nextInt(5) + 2` blocks,
/// stepping outwards after its first block and then with 0.25 once it has
/// climbed at least two blocks since the last step. Always returns `true`.
pub fn place_coral_tree<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let block = coral_block(rng);
    let mut p = origin;
    let trunk = rng.next_int_bounded(3) + 1;
    for _ in 0..trunk {
        if !place_coral_block(world, rng, p, block) {
            return true;
        }
        p = p.above();
    }
    let top = p;
    let branches = (rng.next_int_bounded(3) + 2) as usize;
    let mut dirs = Direction::HORIZONTAL;
    java_shuffle(&mut dirs, rng);
    for &dir in &dirs[..branches] {
        p = top.rel(dir);
        let len = rng.next_int_bounded(5) + 2;
        let mut since_step = 0;
        let mut i = 0;
        while i < len && place_coral_block(world, rng, p, block) {
            since_step += 1;
            p = p.above();
            if i == 0 || (since_step >= 2 && rng.next_float() < 0.25) {
                p = p.rel(dir);
                since_step = 0;
            }
            i += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::{BlockSet, JavaBlock};
    use crate::features::java_blocks::BLOCKS;
    use crate::features::test_world::TestWorld;

    fn tag_members(set: BlockSet) -> usize {
        (0..BLOCKS.len())
            .filter(|i| JavaBlock(*i as u16).is_in(set))
            .count()
    }

    #[test]
    fn coral_lists_match_tags() {
        for (list, set) in [
            (&CORAL_BLOCKS[..], tags::CORAL_BLOCKS),
            (&CORALS[..], tags::CORALS),
            (&WALL_CORALS[..], tags::WALL_CORALS),
        ] {
            assert_eq!(list.len(), tag_members(set));
            assert!(list.iter().all(|s| s.is_in(set)));
            // Tag order equals registry order for these tags.
            assert!(list.windows(2).all(|w| w[0].block() < w[1].block()));
        }
        for s in CORALS.iter().chain(WALL_CORALS.iter()) {
            assert_eq!(s.value("waterlogged"), Some("true"));
        }
    }

    /// Stone below `floor`, water from `floor` up to `surface - 1`.
    fn ocean(floor: i32, surface: i32, radius: i32) -> TestWorld {
        let mut world = TestWorld::flat(floor);
        for x in -radius..=radius {
            for z in -radius..=radius {
                for y in floor..surface {
                    world.blocks.insert(BlockPos::new(x, y, z), states::WATER);
                }
            }
        }
        world
    }

    #[test]
    fn kelp_column_shape() {
        for seed in 0..20 {
            let mut world = ocean(0, 30, 1);
            let mut rng = FeatureRandom::new(seed);
            assert!(place_kelp(&mut world, &mut rng, BlockPos::new(0, 50, 0)));
            let mut y = 0;
            while world.get_state(BlockPos::new(0, y, 0)).is(b::KELP_PLANT) {
                y += 1;
            }
            let head = world.get_state(BlockPos::new(0, y, 0));
            assert!(head.is(b::KELP), "seed {seed}");
            let age = head.int_value("age").unwrap();
            assert!((20..=23).contains(&age));
            assert!((1..=10).contains(&y));
        }
    }

    #[test]
    fn kelp_short_water_column_gets_head_below_surface() {
        let mut world = ocean(0, 3, 1);
        let mut rng = FeatureRandom::new(7);
        assert!(place_kelp(&mut world, &mut rng, BlockPos::new(0, 50, 0)));
        // Water at y 0..=2: plant needs water above, so the head ends at y 1.
        assert!(world.get_state(BlockPos::new(0, 1, 0)).is(b::KELP));
        assert!(world.get_state(BlockPos::new(0, 2, 0)).is(b::WATER));
    }

    #[test]
    fn kelp_needs_water() {
        let mut world = TestWorld::flat(0);
        let mut rng = FeatureRandom::new(0);
        assert!(!place_kelp(&mut world, &mut rng, BlockPos::new(0, 0, 0)));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn seagrass_and_pickles_stay_on_floor() {
        for seed in 0..30 {
            let mut world = ocean(0, 10, 8);
            let mut rng = FeatureRandom::new(seed);
            place_seagrass(&mut world, &mut rng, 0.5, BlockPos::new(0, 0, 0));
            place_sea_pickle(
                &mut world,
                &mut rng,
                &IntProvider::Constant(4),
                BlockPos::new(0, 0, 0),
            );
            for (pos, state) in &world.blocks {
                if state.is(b::WATER) {
                    continue;
                }
                let upper_half = state.is(b::TALL_SEAGRASS) && state.value("half") == Some("upper");
                assert_eq!(
                    pos.y,
                    if upper_half { 1 } else { 0 },
                    "{state:?} at {pos:?}"
                );
                let pickles = state.int_value("pickles");
                if state.is(b::SEA_PICKLE) {
                    assert!((1..=4).contains(&pickles.unwrap()));
                }
            }
        }
    }

    #[test]
    fn coral_shapes_only_replace_water() {
        type Shape = fn(&mut TestWorld, &mut FeatureRandom, BlockPos) -> bool;
        let shapes: [Shape; 3] = [place_coral_claw, place_coral_mushroom, place_coral_tree];
        for (k, shape) in shapes.into_iter().enumerate() {
            for seed in 0..10 {
                let mut world = ocean(0, 20, 12);
                let mut rng = FeatureRandom::new(seed * 3 + k as i64);
                assert!(shape(&mut world, &mut rng, BlockPos::new(0, 2, 0)));
                let mut kinds = std::collections::HashSet::new();
                for (pos, state) in &world.blocks {
                    assert!(pos.y >= 0 && pos.y < 20);
                    if CORAL_BLOCKS.contains(state) {
                        kinds.insert(state.block());
                    }
                }
                assert!(kinds.len() <= 1, "one coral block type per feature");
            }
        }
    }

    #[test]
    fn coral_fails_on_dry_land() {
        let mut world = TestWorld::flat(0);
        let mut rng = FeatureRandom::new(1);
        assert!(!place_coral_claw(
            &mut world,
            &mut rng,
            BlockPos::new(0, 0, 0)
        ));
        assert!(place_coral_tree(
            &mut world,
            &mut rng,
            BlockPos::new(0, 0, 0)
        ));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn coral_deterministic() {
        let run = |seed| {
            let mut world = ocean(0, 20, 12);
            let mut rng = FeatureRandom::new(seed);
            place_coral_claw(&mut world, &mut rng, BlockPos::new(0, 2, 0));
            let mut blocks: Vec<_> = world
                .blocks
                .into_iter()
                .map(|(p, s)| (p.x, p.y, p.z, s.0))
                .collect();
            blocks.sort();
            (blocks, rng.next_int())
        };
        assert_eq!(run(9), run(9));
    }
}
