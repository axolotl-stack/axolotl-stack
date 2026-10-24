//! `minecraft:ore` and `minecraft:scattered_ore`.
//!
//! Java writes ore blocks straight into chunk sections; positions are always
//! inside the 3x3 decoration region (an ore blob reaches at most 13 blocks
//! from its origin, scattered ore 7), so the region write check never skips a
//! position before its rule tests draw.

use crate::features::blocks::{BlockPos, Direction, Heightmap, JavaState};
use crate::features::config::OreConfig;
use crate::features::eval::{FeatureWorld, test_rule};
use crate::features::random::FeatureRandom;

/// Largest `size` the Java codec accepts.
const MAX_SIZE: usize = 64;
/// Bits in the visited-position set. For `size = 64` the blob box is
/// `width = 26`, `height = 14`, and Java's index
/// `dx + dy * width + dz * width * height` stays below
/// `width * (width * height + height + 1) = 9854`.
const VISITED_BITS: usize = 10_240;

/// Java `Mth.sin(double)`: the 65536-entry sine table, indexed by
/// `(long)(v * 65536 / 2pi) & 65535`. The table entry is recomputed instead of
/// stored; it is `(float) Math.sin(i * PI * 2 / 65536)`.
#[inline]
fn mth_sin(v: f64) -> f32 {
    let index = ((v * 10430.378350470453) as i64 & 65535) as f64;
    (index * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

/// Java `Mth.ceil(float)`.
#[inline]
fn ceil_f32(v: f32) -> i32 {
    let i = v as i32;
    if v > i as f32 { i + 1 } else { i }
}

/// Java `OreFeature.shouldSkipAirCheck`: draws a float only for a chance
/// strictly between 0 and 1.
#[inline]
fn should_skip_air_check(rng: &mut FeatureRandom, chance: f32) -> bool {
    if chance <= 0.0 {
        true
    } else if chance >= 1.0 {
        false
    } else {
        rng.next_float() >= chance
    }
}

/// Java `Feature.isAdjacentToAir`: any of the six neighbours (in
/// `Direction.values()` order) is air.
#[inline]
fn is_adjacent_to_air<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    Direction::ALL.iter().any(|&d| world.is_air(pos.rel(d)))
}

/// Java `OreFeature.canPlaceOre`: the target's rule test first (it may draw a
/// float), then the air-exposure discard chance (a float when the chance is in
/// `(0, 1)`), then the neighbour air check.
#[inline]
pub fn can_place_ore<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &OreConfig,
    target: &crate::features::config::OreTarget,
    state: JavaState,
    pos: BlockPos,
) -> bool {
    if !test_rule(&target.target, state, rng) {
        return false;
    }
    should_skip_air_check(rng, cfg.discard_chance_on_air_exposure)
        || !is_adjacent_to_air(world, pos)
}

/// Try every target in order at `pos`; place the first that passes.
#[inline]
fn place_first_target<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &OreConfig,
    pos: BlockPos,
) -> bool {
    let state = world.get_state(pos);
    for target in cfg.targets {
        if can_place_ore(world, rng, cfg, target, state, pos) {
            world.set_state(pos, target.state);
            return true;
        }
    }
    false
}

/// Java `minecraft:ore` (`OreFeature`).
///
/// Draws the blob direction (`nextFloat`) and the two end heights
/// (`nextInt(3)` twice), then bails with `false` unless some column of the
/// blob's bounding box has its `OCEAN_FLOOR_WG` height at or above the box's
/// bottom. The blob is `size` spheres along the segment between the two ends,
/// each with a `nextDouble` radius; spheres contained in an earlier one are
/// dropped. Every position is visited once (Java's bitset, including its index
/// aliasing at the box edge) and gets the first target whose rule passes and
/// that survives the air-exposure discard. Returns whether any block was
/// placed; `size = 0` draws the three header values and places nothing.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &OreConfig,
    origin: BlockPos,
) -> bool {
    let angle = rng.next_float() * std::f32::consts::PI;
    let spread = cfg.size as f32 / 8.0;
    let pad = ceil_f32((cfg.size as f32 / 16.0 * 2.0 + 1.0) / 2.0);
    let sin = (angle as f64).sin() * spread as f64;
    let cos = (angle as f64).cos() * spread as f64;
    let min_x = origin.x as f64 + sin;
    let max_x = origin.x as f64 - sin;
    let min_z = origin.z as f64 + cos;
    let max_z = origin.z as f64 - cos;
    let min_y = (origin.y + rng.next_int_bounded(3) - 2) as f64;
    let max_y = (origin.y + rng.next_int_bounded(3) - 2) as f64;
    let x = origin.x - ceil_f32(spread) - pad;
    let y = origin.y - 2 - pad;
    let z = origin.z - ceil_f32(spread) - pad;
    let width = 2 * (ceil_f32(spread) + pad);
    let height = 2 * (2 + pad);

    for sx in x..=x + width {
        for sz in z..=z + width {
            if y <= world.get_height(Heightmap::OceanFloorWg, sx, sz) {
                let blob = Blob {
                    min_x,
                    max_x,
                    min_z,
                    max_z,
                    min_y,
                    max_y,
                    x,
                    y,
                    z,
                    width,
                    height,
                };
                return do_place(world, rng, cfg, &blob);
            }
        }
    }
    false
}

/// Segment ends and bounding box of one ore blob.
struct Blob {
    min_x: f64,
    max_x: f64,
    min_z: f64,
    max_z: f64,
    min_y: f64,
    max_y: f64,
    x: i32,
    y: i32,
    z: i32,
    width: i32,
    height: i32,
}

/// Java `OreFeature.doPlace`.
fn do_place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &OreConfig,
    blob: &Blob,
) -> bool {
    debug_assert!(
        (0..=MAX_SIZE as i32).contains(&cfg.size),
        "ore size {}",
        cfg.size
    );
    let size = cfg.size as usize;
    // (x, y, z, radius) per sphere.
    let mut spheres = [[0.0f64; 4]; MAX_SIZE];
    for (k, sphere) in spheres.iter_mut().enumerate().take(size) {
        let f = k as f32 / size as f32;
        let cx = lerp(f as f64, blob.min_x, blob.max_x);
        let cy = lerp(f as f64, blob.min_y, blob.max_y);
        let cz = lerp(f as f64, blob.min_z, blob.max_z);
        let r = rng.next_double() * size as f64 / 16.0;
        let radius = ((mth_sin((std::f32::consts::PI * f) as f64) + 1.0) as f64 * r + 1.0) / 2.0;
        *sphere = [cx, cy, cz, radius];
    }

    for k in 0..size.saturating_sub(1) {
        if spheres[k][3] <= 0.0 {
            continue;
        }
        for m in k + 1..size {
            if spheres[m][3] <= 0.0 {
                continue;
            }
            let dx = spheres[k][0] - spheres[m][0];
            let dy = spheres[k][1] - spheres[m][1];
            let dz = spheres[k][2] - spheres[m][2];
            let dr = spheres[k][3] - spheres[m][3];
            if dr * dr > dx * dx + dy * dy + dz * dz {
                if dr > 0.0 {
                    spheres[m][3] = -1.0;
                } else {
                    spheres[k][3] = -1.0;
                }
            }
        }
    }

    let mut visited = [0u64; VISITED_BITS / 64];
    let mut placed = 0;
    for &[cx, cy, cz, radius] in spheres.iter().take(size) {
        if radius < 0.0 {
            continue;
        }
        let x0 = ((cx - radius).floor() as i32).max(blob.x);
        let y0 = ((cy - radius).floor() as i32).max(blob.y);
        let z0 = ((cz - radius).floor() as i32).max(blob.z);
        let x1 = ((cx + radius).floor() as i32).max(x0);
        let y1 = ((cy + radius).floor() as i32).max(y0);
        let z1 = ((cz + radius).floor() as i32).max(z0);
        for bx in x0..=x1 {
            let u = (bx as f64 + 0.5 - cx) / radius;
            if u * u >= 1.0 {
                continue;
            }
            for by in y0..=y1 {
                let w = (by as f64 + 0.5 - cy) / radius;
                if u * u + w * w >= 1.0 {
                    continue;
                }
                for bz in z0..=z1 {
                    let v = (bz as f64 + 0.5 - cz) / radius;
                    if u * u + w * w + v * v >= 1.0 || world.is_outside_build_height(by) {
                        continue;
                    }
                    let index = (bx - blob.x
                        + (by - blob.y) * blob.width
                        + (bz - blob.z) * blob.width * blob.height)
                        as usize;
                    let (word, bit) = (index / 64, 1u64 << (index % 64));
                    if visited[word] & bit != 0 {
                        continue;
                    }
                    visited[word] |= bit;
                    if place_first_target(world, rng, cfg, BlockPos::new(bx, by, bz)) {
                        placed += 1;
                    }
                }
            }
        }
    }
    placed > 0
}

/// Java `Mth.lerp(double, double, double)`.
#[inline]
fn lerp(delta: f64, start: f64, end: f64) -> f64 {
    start + delta * (end - start)
}

/// Java `ScatteredOreFeature.MAX_DIST_FROM_ORIGIN`.
const SCATTER_MAX_DIST: i32 = 7;

/// Java `minecraft:scattered_ore` (`ScatteredOreFeature`).
///
/// `nextInt(size + 1)` attempts; attempt `j` is offset from the origin on each
/// axis (x, y, z) by `round((nextFloat() - nextFloat()) * min(j, 7))` and gets
/// the first target that passes `can_place_ore`. Always returns `true`.
pub fn place_scattered<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &OreConfig,
    origin: BlockPos,
) -> bool {
    let attempts = rng.next_int_bounded(cfg.size + 1);
    for j in 0..attempts {
        let range = j.min(SCATTER_MAX_DIST);
        let dx = scatter_offset(rng, range);
        let dy = scatter_offset(rng, range);
        let dz = scatter_offset(rng, range);
        place_first_target(world, rng, cfg, origin.offset(dx, dy, dz));
    }
    true
}

/// `getRandomPlacementInOneAxisRelativeToOrigin`: Java `Math.round(float)`
/// (round half up) of `(nextFloat() - nextFloat()) * range`.
#[inline]
fn scatter_offset(rng: &mut FeatureRandom, range: i32) -> i32 {
    let a = rng.next_float();
    let b = rng.next_float();
    let v = (a - b) * range as f32;
    // A float plus 0.5 is exact in double precision.
    (v as f64 + 0.5).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::RuleTest;
    use crate::features::config::OreTarget;
    use crate::features::java_blocks::{states, tags};
    use crate::features::test_world::TestWorld;

    static STONE_TARGETS: [OreTarget; 2] = [
        OreTarget {
            target: RuleTest::TagMatch(tags::STONE_ORE_REPLACEABLES),
            state: states::IRON_ORE,
        },
        OreTarget {
            target: RuleTest::TagMatch(tags::DEEPSLATE_ORE_REPLACEABLES),
            state: states::DEEPSLATE_IRON_ORE,
        },
    ];

    fn cfg(size: i32, discard: f32) -> OreConfig {
        OreConfig {
            targets: &STONE_TARGETS,
            size,
            discard_chance_on_air_exposure: discard,
        }
    }

    fn run(seed: i64, cfg: &OreConfig, world: &mut TestWorld, origin: BlockPos) -> bool {
        let mut rng = FeatureRandom::new(seed);
        place(world, &mut rng, cfg, origin)
    }

    #[test]
    fn mth_sin_matches_table_points() {
        assert_eq!(mth_sin(0.0), 0.0);
        assert_eq!(mth_sin(std::f64::consts::FRAC_PI_2), 1.0);
        assert!(mth_sin(std::f64::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn ore_only_replaces_targets_inside_blob() {
        let origin = BlockPos::new(8, 0, 8);
        for seed in 0..32 {
            let mut world = TestWorld::flat(64);
            // A dirt slab the targets must leave alone.
            for x in -8..24 {
                for z in -8..24 {
                    world.set_state(BlockPos::new(x, 1, z), states::DIRT);
                }
            }
            let placed = run(seed, &cfg(17, 0.0), &mut world, origin);
            assert!(placed, "seed {seed}");
            for (pos, state) in &world.blocks {
                if pos.y == 1 {
                    assert_eq!(*state, states::DIRT, "seed {seed} replaced dirt at {pos:?}");
                } else {
                    assert_eq!(*state, states::IRON_ORE, "seed {seed} at {pos:?}");
                    assert!((pos.x - origin.x).abs() <= 6 && (pos.z - origin.z).abs() <= 6);
                    assert!((pos.y - origin.y).abs() <= 6);
                }
            }
        }
    }

    #[test]
    fn deterministic_for_seed() {
        let mut a = TestWorld::flat(64);
        let mut b = TestWorld::flat(64);
        let origin = BlockPos::new(3, 10, -5);
        run(77, &cfg(12, 0.5), &mut a, origin);
        run(77, &cfg(12, 0.5), &mut b, origin);
        assert_eq!(a.blocks, b.blocks);
        assert!(a.count(states::IRON_ORE) > 0);
    }

    #[test]
    fn size_zero_places_nothing() {
        let mut world = TestWorld::flat(64);
        assert!(!run(5, &cfg(0, 0.0), &mut world, BlockPos::new(0, 0, 0)));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn bails_above_ocean_floor() {
        // Surface at y=0; a blob centred at y=40 has its box bottom above
        // every column's ocean-floor height.
        let mut world = TestWorld::flat(0);
        assert!(!run(5, &cfg(9, 0.0), &mut world, BlockPos::new(0, 40, 0)));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn full_air_discard_skips_exposed_blocks() {
        // Surface at y=0, blob straddling it: with discard chance 1 no ore may
        // touch air.
        for seed in 0..16 {
            let mut world = TestWorld::flat(0);
            run(seed, &cfg(20, 1.0), &mut world, BlockPos::new(0, 0, 0));
            for (pos, _) in world.blocks.clone() {
                assert!(
                    !is_adjacent_to_air(&mut world, pos),
                    "seed {seed} at {pos:?}"
                );
            }
        }
    }

    #[test]
    fn scattered_stays_within_spread() {
        let origin = BlockPos::new(0, 0, 0);
        for seed in 0..32 {
            let mut world = TestWorld::flat(64);
            let mut rng = FeatureRandom::new(seed);
            assert!(place_scattered(&mut world, &mut rng, &cfg(20, 0.0), origin));
            for (pos, state) in &world.blocks {
                assert_eq!(*state, states::IRON_ORE);
                assert!(pos.x.abs() <= 7 && pos.y.abs() <= 7 && pos.z.abs() <= 7);
            }
            assert!(world.blocks.len() <= 20);
        }
    }
}
