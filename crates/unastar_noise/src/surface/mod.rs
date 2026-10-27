//! Runtime side of the compiled surface rules.
//!
//! `build.rs` compiles `noise_settings/overworld.json`'s `surface_rule` into straight-line
//! Rust (`crate::overworld_surface::apply`). The generated code reads the column state it
//! needs through [`SurfaceEnv`], which the generator's surface system implements (Java's
//! surface-rule context: stone depths, water height, surface depth, lazily sampled biome,
//! noises, gradients, steepness and preliminary surface).

use crate::NoiseRef;
use crate::java_biomes::JavaBiome;
use crate::xoroshiro::Xoroshiro128;

/// Column and block state a surface rule reads; positions are the block being tested.
pub trait SurfaceEnv {
    /// Block y.
    fn block_y(&self) -> i32;
    /// `(int)(surface noise * 2.75 + 3 + jitter)` of the column.
    fn surface_depth(&self) -> i32;
    /// The secondary surface noise of the column.
    fn surface_secondary(&mut self) -> f64;
    /// Solid blocks from this block up to (and including) the nearest air or fluid above.
    fn stone_depth_above(&self) -> i32;
    /// Solid blocks from this block down to the nearest non-solid block below.
    fn stone_depth_below(&self) -> i32;
    /// y of the first non-fluid block above the nearest fluid above; `i32::MIN` if none.
    fn water_height(&self) -> i32;
    /// The (zoomed) biome at this block.
    fn biome(&mut self) -> JavaBiome;
    /// `noise` sampled at the column (`x, 0, z`).
    fn noise(&mut self, noise: NoiseRef) -> f64;
    /// `nextFloat()` of `fromHashOf(name).forkPositional().at(x, y, z)` for the gradient
    /// with this index in `overworld_surface::GRADIENT_HASHES`.
    fn gradient_float(&mut self, gradient: usize) -> f32;
    /// The column's slope test on the generation surface heightmap.
    fn steep(&mut self) -> bool;
    /// The interpolated preliminary surface level minus 8 plus the surface depth.
    fn min_surface_level(&mut self) -> i32;
    /// Whether the biome at this block is cold enough to snow.
    fn cold_enough_to_snow(&mut self) -> bool;
    /// The badlands terracotta band at this block (canonical Java state id).
    fn band(&mut self) -> u32;
}

/// Java's `Mth.map(value, from_min, from_max, to_min, to_max)`.
#[inline]
pub fn map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    to_min + (value - from_min) / (from_max - from_min) * (to_max - to_min)
}

/// Canonical Java state ids of the terracotta colours the clay bands use.
#[derive(Debug, Clone, Copy)]
pub struct BandBlocks {
    pub terracotta: u32,
    pub orange: u32,
    pub yellow: u32,
    pub brown: u32,
    pub red: u32,
    pub white: u32,
    pub light_gray: u32,
}

/// `minecraft:stone_depth`: the stone depth above (floor) or below (ceiling) is at most
/// `1 + offset`, plus the surface depth and a secondary-noise range when asked.
#[inline]
pub fn stone_depth<E: SurfaceEnv>(
    env: &mut E,
    ceiling: bool,
    offset: i32,
    add_surface_depth: bool,
    secondary_depth_range: i32,
) -> bool {
    let depth = if ceiling {
        env.stone_depth_below()
    } else {
        env.stone_depth_above()
    };
    let surface = if add_surface_depth {
        env.surface_depth()
    } else {
        0
    };
    let secondary = if secondary_depth_range == 0 {
        0
    } else {
        map(
            env.surface_secondary(),
            -1.0,
            1.0,
            0.0,
            secondary_depth_range as f64,
        ) as i32
    };
    depth <= 1 + offset + surface + secondary
}

/// `minecraft:y_above`.
#[inline]
pub fn y_above<E: SurfaceEnv>(env: &mut E, y: i32, multiplier: i32, add_stone_depth: bool) -> bool {
    let stone = if add_stone_depth {
        env.stone_depth_above()
    } else {
        0
    };
    env.block_y() + stone >= y + env.surface_depth() * multiplier
}

/// `minecraft:water`: no fluid above, or this block is high enough relative to it.
#[inline]
pub fn water<E: SurfaceEnv>(
    env: &mut E,
    offset: i32,
    multiplier: i32,
    add_stone_depth: bool,
) -> bool {
    let water = env.water_height();
    if water == i32::MIN {
        return true;
    }
    let stone = if add_stone_depth {
        env.stone_depth_above()
    } else {
        0
    };
    env.block_y() + stone >= water + offset + env.surface_depth() * multiplier
}

/// `minecraft:noise_threshold`: the column's noise value lies in `[min, max]`.
#[inline]
pub fn noise_threshold<E: SurfaceEnv>(env: &mut E, noise: NoiseRef, min: f64, max: f64) -> bool {
    let value = env.noise(noise);
    value >= min && value <= max
}

/// `minecraft:vertical_gradient`: always below `low`, never at or above `high`, linear odds
/// in between from the gradient's positional random.
#[inline]
pub fn vertical_gradient<E: SurfaceEnv>(env: &mut E, gradient: usize, low: i32, high: i32) -> bool {
    let y = env.block_y();
    if y <= low {
        return true;
    }
    if y >= high {
        return false;
    }
    let chance = map(y as f64, low as f64, high as f64, 1.0, 0.0);
    (env.gradient_float(gradient) as f64) < chance
}

/// Number of clay bands.
pub const BAND_COUNT: usize = 192;

fn next_int_between_inclusive(random: &mut Xoroshiro128, min: u32, max: u32) -> u32 {
    random.next_int(max - min + 1) + min
}

fn make_bands(
    random: &mut Xoroshiro128,
    bands: &mut [u32; BAND_COUNT],
    min_width: u32,
    block: u32,
) {
    let count = next_int_between_inclusive(random, 6, 15);
    for _ in 0..count {
        let width = (min_width + random.next_int(3)) as usize;
        let start = random.next_int(BAND_COUNT as u32) as usize;
        for m in 0..width {
            if start + m >= BAND_COUNT {
                break;
            }
            bands[start + m] = block;
        }
    }
}

/// The badlands terracotta bands, drawn from `fromHashOf("minecraft:clay_bands")`.
pub fn clay_bands(random: &mut Xoroshiro128, blocks: &BandBlocks) -> [u32; BAND_COUNT] {
    let mut bands = [blocks.terracotta; BAND_COUNT];
    let mut i = 0usize;
    // The native for-loop checks its cursor before drawing the next orange gap.
    // If an orange band lands at 191, the loop increment reaches 192 exactly:
    // another draw would shift every subsequent colour's random stream.
    while i < BAND_COUNT {
        i += random.next_int(5) as usize + 1;
        if i >= BAND_COUNT {
            break;
        }
        bands[i] = blocks.orange;
        i += 1;
    }
    make_bands(random, &mut bands, 1, blocks.yellow);
    make_bands(random, &mut bands, 2, blocks.brown);
    make_bands(random, &mut bands, 1, blocks.red);
    let white_count = next_int_between_inclusive(random, 9, 15);
    let mut placed = 0;
    let mut k = 0usize;
    while placed < white_count && k < BAND_COUNT {
        bands[k] = blocks.white;
        if k > 1 && random.next_boolean() {
            bands[k - 1] = blocks.light_gray;
        }
        if k + 1 < BAND_COUNT && random.next_boolean() {
            bands[k + 1] = blocks.light_gray;
        }
        placed += 1;
        k += random.next_int(16) as usize + 4;
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xoroshiro::PositionalRandomFactory;

    #[test]
    fn native_clay_bands_orange_cursor_reaches_array_end() {
        // Actual pinned 1.21.11 SurfaceSystem.clayBands, captured with the
        // constructor-only tools/nativebadlandsprobe/NativeBadlandsOracle.java.
        // Delegating native generateBands through RandomSource reproduced the
        // constructed array exactly: 53 orange-gap draws end at cursor 192,
        // followed immediately by nextIntBetweenInclusive(6, 15) == 14.
        let factory = PositionalRandomFactory::new(-734287144591);
        let mut random = factory.from_hash(crate::random_hashes::CLAY_BANDS);
        let blocks = BandBlocks {
            terracotta: 0,
            orange: 1,
            yellow: 2,
            brown: 3,
            red: 4,
            white: 5,
            light_gray: 6,
        };
        let native = [
            5, 6, 1, 0, 0, 0, 0, 0, 1, 0, 1, 6, 5, 6, 0, 0,
            1, 6, 5, 6, 4, 2, 3, 3, 0, 0, 0, 0, 0, 1, 0, 0,
            0, 0, 5, 2, 0, 1, 0, 6, 5, 6, 0, 0, 1, 4, 4, 2,
            3, 3, 6, 5, 6, 3, 3, 3, 5, 6, 0, 0, 1, 0, 1, 0,
            1, 0, 0, 0, 1, 0, 0, 6, 5, 6, 3, 3, 3, 0, 0, 1,
            0, 2, 1, 0, 0, 0, 2, 0, 0, 0, 0, 5, 1, 0, 2, 2,
            0, 1, 0, 2, 2, 0, 0, 1, 0, 4, 4, 6, 5, 6, 1, 0,
            0, 0, 1, 0, 0, 4, 4, 1, 0, 0, 0, 2, 2, 5, 6, 3,
            3, 3, 3, 4, 0, 0, 0, 1, 0, 1, 3, 3, 0, 0, 0, 5,
            6, 3, 3, 0, 0, 1, 6, 5, 6, 0, 1, 0, 0, 3, 3, 3,
            2, 0, 4, 5, 1, 0, 1, 4, 4, 1, 0, 0, 0, 1, 0, 0,
            2, 4, 5, 6, 0, 0, 4, 4, 4, 2, 2, 1, 0, 3, 3, 2,
        ];
        assert_eq!(clay_bands(&mut random, &blocks), native);
    }
}
