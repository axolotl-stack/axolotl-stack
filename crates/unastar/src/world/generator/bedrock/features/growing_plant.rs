//! `minecraft:growing_plant_feature` (cave vines, big dripleaf, pale hanging
//! moss): a column of body blocks capped by a head block, grown from the
//! input along `growth_direction`.
//!
//! The height comes from `height_distribution`: one range is picked by weight,
//! then a height drawn uniformly from it. The column starts at the input and
//! stops early when the next cell is not open (air, or water with
//! `allow_water`); the last block placed is always a head. Vanilla (Java)
//! only tests the next cell for air even with `allow_water`; here water
//! counts as open too, so a dripleaf grows through a pool to its full height
//! instead of ending at its first submerged cell. The input itself must be
//! open, otherwise nothing is placed.
//!
//! The decoration world has a single block layer: a plant grown through water
//! replaces the water (BDS waterlogs it).

use super::super::generated::bb;
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx};
use super::IntRange;

/// Parameters of one `minecraft:growing_plant_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `height_distribution`: `(height range, weight)`.
    pub heights: &'static [(IntRange, i32)],
    /// `growth_direction`: `1` up, `-1` down.
    pub step: i32,
    /// `allow_water`: water cells count as open.
    pub allow_water: bool,
    /// `body_blocks`: `(state, weight)`.
    pub body: &'static [(u32, i32)],
    /// `head_blocks`: `(states, weight)`; `states` holds one state per
    /// drawable `age` (uniform pick), or the single state of an ageless head.
    pub head: &'static [(&'static [u32], i32)],
}

/// Pick an item by weight (`items` must not be empty).
pub fn pick_weighted<T: Copy>(items: &[(T, i32)], rng: &mut BedrockRandom) -> T {
    let total: i32 = items.iter().map(|(_, w)| *w).sum();
    let mut r = rng.next_int(total);
    for &(item, w) in items {
        if r < w {
            return item;
        }
        r -= w;
    }
    items[items.len() - 1].0
}

/// Draw a column height from a weighted `height_distribution`.
pub fn sample_height(heights: &[(IntRange, i32)], rng: &mut BedrockRandom) -> i32 {
    pick_weighted(heights, rng).sample(rng)
}

#[inline]
fn is_open(id: u32, allow_water: bool) -> bool {
    bb::air(id) || (allow_water && bb::water(id))
}

/// Grow one plant from `pos`; `Some(pos)` when any block was placed.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    let height = sample_height(p.heights, ctx.rng);
    let mut at = pos;
    let mut placed = false;
    for i in 1..=height {
        if !is_open(ctx.get(at), p.allow_water) {
            break;
        }
        let next = at.offset(0, p.step, 0);
        let head = i == height || !is_open(ctx.get(next), p.allow_water);
        let id = if head {
            let states = pick_weighted(p.head, ctx.rng);
            if states.len() == 1 {
                states[0]
            } else {
                states[ctx.rng.next_int(states.len() as i32) as usize]
            }
        } else {
            pick_weighted(p.body, ctx.rng)
        };
        if !ctx.set(at, id) {
            break;
        }
        placed = true;
        if head {
            break;
        }
        at = next;
    }
    placed.then_some(pos)
}

#[cfg(test)]
mod tests {
    use super::super::multiface::test_world::{Cube, with_ctx};
    use super::*;

    static HEADS: [u32; 2] = [bb::GRAVEL, bb::CLAY];
    static PLANT: Params = Params {
        heights: &[(IntRange { min: 6, max: 6 }, 1)],
        step: -1,
        allow_water: false,
        body: &[(bb::SAND, 1)],
        head: &[(&HEADS, 1)],
    };

    #[test]
    fn column_stops_above_the_floor_with_a_head() {
        // Hanging from y = 3 down in a 4-block-high cave: 4 blocks fit.
        for seed in 0..16 {
            let mut world = Cube::filled(8, bb::STONE);
            world.fill((0, 0, 0), (0, 3, 0), bb::AIR);
            let mut rng = BedrockRandom::new(seed);
            let placed = with_ctx(&mut world, &mut rng, |ctx| {
                place(ctx, BlockPos::new(0, 3, 0), &PLANT)
            });
            assert_eq!(placed, Some(BlockPos::new(0, 3, 0)));
            for y in 1..=3 {
                assert_eq!(
                    world.get(BlockPos::new(0, y, 0)),
                    bb::SAND,
                    "body at y = {y}"
                );
            }
            assert!(
                HEADS.contains(&world.get(BlockPos::new(0, 0, 0))),
                "head on the last open cell"
            );
        }
    }

    #[test]
    fn full_height_column_ends_with_a_head() {
        let mut world = Cube::filled(8, bb::AIR);
        let mut rng = BedrockRandom::new(2);
        with_ctx(&mut world, &mut rng, |ctx| {
            place(ctx, BlockPos::new(0, 7, 0), &PLANT)
        })
        .expect("placed");
        assert_eq!(world.count(bb::SAND), 5);
        assert!(HEADS.contains(&world.get(BlockPos::new(0, 2, 0))));
        assert_eq!(world.get(BlockPos::new(0, 1, 0)), bb::AIR);
    }

    #[test]
    fn water_is_open_only_with_allow_water() {
        let wet = Params {
            allow_water: true,
            step: 1,
            ..PLANT
        };
        let dry = Params { step: 1, ..PLANT };
        for (p, expected) in [(wet, 6), (dry, 0)] {
            let mut world = Cube::filled(8, bb::WATER);
            let mut rng = BedrockRandom::new(3);
            let placed = with_ctx(&mut world, &mut rng, |ctx| {
                place(ctx, BlockPos::new(0, 0, 0), &p)
            });
            assert_eq!(placed.is_some(), expected > 0);
            assert_eq!(
                world.count(bb::SAND) + world.count(bb::GRAVEL) + world.count(bb::CLAY),
                expected
            );
        }
    }

    #[test]
    fn weighted_pick_follows_weights() {
        let mut rng = BedrockRandom::new(1);
        let items = [('a', 1), ('b', 3), ('c', 0)];
        let mut counts = [0u32; 3];
        for _ in 0..40_000 {
            match pick_weighted(&items, &mut rng) {
                'a' => counts[0] += 1,
                'b' => counts[1] += 1,
                _ => counts[2] += 1,
            }
        }
        assert_eq!(counts[2], 0, "zero weight is never picked");
        let ratio = counts[1] as f64 / counts[0] as f64;
        assert!((2.7..3.3).contains(&ratio), "b/a = {ratio}");
    }

    #[test]
    fn height_stays_in_the_ranges_and_covers_them() {
        // cave_vine_feature's distribution: 1..13 (2), 1..2 (3), 1..7 (10).
        let heights = [
            (IntRange { min: 1, max: 13 }, 2),
            (IntRange { min: 1, max: 2 }, 3),
            (IntRange { min: 1, max: 7 }, 10),
        ];
        let mut rng = BedrockRandom::new(99);
        let mut seen = [0u32; 14];
        for _ in 0..20_000 {
            let h = sample_height(&heights, &mut rng);
            assert!((1..=13).contains(&h));
            seen[h as usize] += 1;
        }
        assert!(
            seen[1..].iter().all(|&c| c > 0),
            "every height occurs: {seen:?}"
        );
        // Heights above 7 only come from the weight-2 range: rarer than 1..=2.
        assert!(seen[8..].iter().sum::<u32>() < seen[1] + seen[2]);
    }

    #[test]
    fn single_item_needs_no_luck() {
        let mut rng = BedrockRandom::new(5);
        for _ in 0..100 {
            assert_eq!(pick_weighted(&[(42u32, 7)], &mut rng), 42);
        }
    }
}
