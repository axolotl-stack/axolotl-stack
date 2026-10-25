//! Ore vein generation system for underground ore veins with filler blocks.
//!
//! This module implements Java Edition's OreVeinifier system which creates
//! large ore veins (copper and iron) with associated filler blocks (granite
//! and tuff respectively). This is where tuff and granite underground come from!
//!
//! ## Vein Types
//!
//! | Type   | Ore Block           | Raw Ore Block    | Filler Block | Y Range  |
//! |--------|---------------------|------------------|--------------|----------|
//! | COPPER | COPPER_ORE          | RAW_COPPER_BLOCK | GRANITE      | 0 to 50  |
//! | IRON   | DEEPSLATE_IRON_ORE  | RAW_IRON_BLOCK   | TUFF         | -60 to -8|
//!
//! ## Algorithm Overview
//!
//! The vein system uses three noise functions from the NoiseRouter:
//! - `vein_toggle` - Determines which vein type (positive = copper, negative = iron)
//! - `vein_ridged` - Creates ridged patterns for vein shape
//! - `vein_gap` - Adds gaps/breaks in veins
//!
//! At each position:
//! 1. Check if within Y range for a vein type
//! 2. Apply edge roundoff near Y boundaries
//! 3. Check veininess threshold (0.4)
//! 4. Random solidness check (0.7)
//! 5. Check ridged noise for vein shape
//! 6. Determine ore vs filler based on richness and gap noise

use std::cell::RefCell;

use crate::world::chunk::{MIN_Y, blocks};
use crate::world::generator::density::{
    FunctionContext, NoiseRef, NoiseRegistry, NoiseSource, compute_vein_gap, compute_vein_toggle,
};
use crate::world::generator::xoroshiro::PositionalRandomFactory;

// ========== Cell interpolation ==========

/// Noise cell size: `vein_toggle` and both halves of `vein_ridged` are `interpolated`
/// in the noise router, so blocks read a trilinear blend of the cell's corner values.
const CELL_WIDTH: i32 = 4;
const CELL_HEIGHT: i32 = 8;
/// Corner rows from the bottom of the world up to the top of the cell holding y = 50.
const CORNER_ROWS: usize = ((50 - MIN_Y) / CELL_HEIGHT + 2) as usize;
/// Corners per axis across a chunk.
const CORNERS_XZ: usize = (16 / CELL_WIDTH + 1) as usize;
/// `ore_vein_a`/`ore_vein_b` are sampled at four times the block coordinate.
const VEIN_RIDGED_SCALE: f64 = 4.0;
/// Offset added to the larger of the two ridged noises.
const VEIN_RIDGED_OFFSET: f64 = -0.07999999821186066;

/// Corner values `[toggle, vein_a, vein_b]` of every noise cell of one chunk.
struct VeinCorners {
    chunk_x: i32,
    chunk_z: i32,
    /// Indexed `[(x * CORNER_ROWS + y) * CORNERS_XZ + z]`.
    values: Vec<[f64; 3]>,
}

impl VeinCorners {
    fn new(chunk_x: i32, chunk_z: i32, noises: &NoiseRegistry) -> Self {
        let mut values = Vec::with_capacity(CORNERS_XZ * CORNER_ROWS * CORNERS_XZ);
        for cx in 0..CORNERS_XZ as i32 {
            let x = chunk_x * 16 + cx * CELL_WIDTH;
            for cy in 0..CORNER_ROWS as i32 {
                let y = MIN_Y + cy * CELL_HEIGHT;
                for cz in 0..CORNERS_XZ as i32 {
                    let z = chunk_z * 16 + cz * CELL_WIDTH;
                    let toggle = compute_vein_toggle(&FunctionContext::new(x, y, z), noises);
                    // `range_choice(y, -60, 51, noise, 0)` around each ridged noise.
                    let ridged = |noise| {
                        if (-60..51).contains(&y) {
                            noises.sample(
                                noise,
                                x as f64 * VEIN_RIDGED_SCALE,
                                y as f64 * VEIN_RIDGED_SCALE,
                                z as f64 * VEIN_RIDGED_SCALE,
                            )
                        } else {
                            0.0
                        }
                    };
                    values.push([
                        toggle,
                        ridged(NoiseRef::OreVeinA),
                        ridged(NoiseRef::OreVeinB),
                    ]);
                }
            }
        }
        Self {
            chunk_x,
            chunk_z,
            values,
        }
    }

    #[inline]
    fn corner(&self, x: usize, y: usize, z: usize) -> &[f64; 3] {
        &self.values[(x * CORNER_ROWS + y) * CORNERS_XZ + z]
    }

    /// Interpolated `[toggle, vein_a, vein_b]` at a block of this chunk, blended along y,
    /// then x, then z like the noise chunk's cell interpolators.
    fn sample(&self, x: i32, y: i32, z: i32) -> [f64; 3] {
        let (local_x, local_y, local_z) = (x - self.chunk_x * 16, y - MIN_Y, z - self.chunk_z * 16);
        let (cell_x, cell_y, cell_z) = (
            (local_x / CELL_WIDTH) as usize,
            (local_y / CELL_HEIGHT) as usize,
            (local_z / CELL_WIDTH) as usize,
        );
        let tx = (local_x % CELL_WIDTH) as f64 / CELL_WIDTH as f64;
        let ty = (local_y % CELL_HEIGHT) as f64 / CELL_HEIGHT as f64;
        let tz = (local_z % CELL_WIDTH) as f64 / CELL_WIDTH as f64;
        let c = |dx, dy, dz| self.corner(cell_x + dx, cell_y + dy, cell_z + dz);
        std::array::from_fn(|i| {
            let xz00 = lerp(ty, c(0, 0, 0)[i], c(0, 1, 0)[i]);
            let xz10 = lerp(ty, c(1, 0, 0)[i], c(1, 1, 0)[i]);
            let xz01 = lerp(ty, c(0, 0, 1)[i], c(0, 1, 1)[i]);
            let xz11 = lerp(ty, c(1, 0, 1)[i], c(1, 1, 1)[i]);
            let z0 = lerp(tx, xz00, xz10);
            let z1 = lerp(tx, xz01, xz11);
            lerp(tz, z0, z1)
        })
    }
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

// ========== Constants from Java ==========

/// Veininess threshold - noise must exceed this for a vein to exist.
const VEININESS_THRESHOLD: f64 = 0.4;

/// Number of blocks from Y boundary where edge roundoff begins.
const EDGE_ROUNDOFF_BEGIN: i32 = 20;

/// Maximum edge roundoff value applied near Y boundaries.
const MAX_EDGE_ROUNDOFF: f64 = 0.2;

/// Probability of placing solid block in vein (vs skipping).
const VEIN_SOLIDNESS: f32 = 0.7;

/// Minimum richness for ore placement (at veininess threshold).
const MIN_RICHNESS: f64 = 0.1;

/// Maximum richness for ore placement (at high veininess).
const MAX_RICHNESS: f64 = 0.3;

/// Veininess threshold for maximum richness.
const MAX_RICHNESS_THRESHOLD: f64 = 0.6;

/// Chance of placing raw ore block instead of normal ore.
const CHANCE_OF_RAW_ORE_BLOCK: f32 = 0.02;

/// Gap noise threshold below which ore is skipped.
const SKIP_ORE_IF_GAP_NOISE_IS_BELOW: f64 = -0.3;

// ========== Vein Types ==========

/// Type of ore vein with associated blocks and Y range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VeinType {
    /// Copper veins with granite filler (Y: 0 to 50).
    Copper,
    /// Iron veins with tuff filler (Y: -60 to -8).
    Iron,
}

impl VeinType {
    /// Get the ore block for this vein type.
    pub fn ore(&self) -> u32 {
        match self {
            VeinType::Copper => *blocks::COPPER_ORE,
            VeinType::Iron => *blocks::DEEPSLATE_IRON_ORE,
        }
    }

    /// Get the raw ore block for this vein type.
    pub fn raw_ore_block(&self) -> u32 {
        match self {
            VeinType::Copper => *blocks::RAW_COPPER_BLOCK,
            VeinType::Iron => *blocks::RAW_IRON_BLOCK,
        }
    }

    /// Get the filler block for this vein type.
    pub fn filler(&self) -> u32 {
        match self {
            VeinType::Copper => *blocks::GRANITE,
            VeinType::Iron => *blocks::TUFF,
        }
    }

    /// Get the minimum Y coordinate for this vein type.
    pub fn min_y(&self) -> i32 {
        match self {
            VeinType::Copper => 0,
            VeinType::Iron => -60,
        }
    }

    /// Get the maximum Y coordinate for this vein type.
    pub fn max_y(&self) -> i32 {
        match self {
            VeinType::Copper => 50,
            VeinType::Iron => -8,
        }
    }
}

// ========== Ore Veinifier ==========

/// Ore vein filler that generates copper/iron veins with granite/tuff filler.
///
/// This is a BlockStateFiller that gets chained after the aquifer in the
/// material rule list. It returns `Some(block)` to override the block at
/// a position, or `None` to let other fillers handle it.
///
/// `vein_toggle` and the two ridged noises are cell-interpolated like the final
/// density: their corner values are sampled once per chunk and blended per block;
/// `vein_gap` is sampled at the block.
pub struct OreVeinifier<'a> {
    /// Noise registry for computing density functions.
    noises: &'a NoiseRegistry,
    /// Positional random factory for ore generation.
    /// This matches Java's `RandomState.oreRandom()`.
    ore_random: PositionalRandomFactory,
    /// Cell corners of the chunk being filled, sampled on the first query in it.
    corners: RefCell<Option<VeinCorners>>,
}

impl<'a> OreVeinifier<'a> {
    /// Create a new ore veinifier using AOT-compiled density functions.
    ///
    /// # Arguments
    /// * `noises` - Noise registry for computing density functions
    /// * `ore_random` - Positional random factory for ore RNG (from `PositionalRandomFactory::fork_ore_random()`)
    pub fn new(noises: &'a NoiseRegistry, ore_random: PositionalRandomFactory) -> Self {
        Self {
            noises,
            ore_random,
            corners: RefCell::new(None),
        }
    }

    /// Interpolated `[toggle, vein_a, vein_b]` at a block.
    fn interpolated(&self, x: i32, y: i32, z: i32) -> [f64; 3] {
        let (chunk_x, chunk_z) = (x >> 4, z >> 4);
        let mut corners = self.corners.borrow_mut();
        let corners = match &mut *corners {
            Some(c) if c.chunk_x == chunk_x && c.chunk_z == chunk_z => c,
            slot => slot.insert(VeinCorners::new(chunk_x, chunk_z, self.noises)),
        };
        corners.sample(x, y, z)
    }

    /// Compute the block state at a position, if in an ore vein.
    ///
    /// Returns `Some(block_id)` if this position is part of an ore vein,
    /// or `None` if no vein block should be placed (let other fillers handle).
    ///
    /// # Arguments
    /// * `ctx` - Function context with block position
    pub fn compute(&self, ctx: &FunctionContext) -> Option<u32> {
        let y = ctx.block_y;

        // Fast early exit: no ore veins possible outside the combined Y range
        // Iron: -60 to -8, Copper: 0 to 50
        // Combined range: -60 to 50
        if !(-60..=50).contains(&y) {
            return None;
        }

        // Step 1: Interpolated vein toggle decides the vein type
        let [toggle_value, vein_a, vein_b] = self.interpolated(ctx.block_x, y, ctx.block_z);

        // Determine vein type from toggle sign
        let vein_type = if toggle_value > 0.0 {
            VeinType::Copper
        } else {
            VeinType::Iron
        };

        // Step 2: Check Y range for this vein type
        let dist_to_max = vein_type.max_y() - y;
        let dist_to_min = y - vein_type.min_y();

        // Outside Y range - no vein
        if dist_to_min < 0 || dist_to_max < 0 {
            return None;
        }

        // Step 3: Calculate edge roundoff
        let min_dist = dist_to_min.min(dist_to_max);
        let edge_roundoff = clamp_map(
            min_dist as f64,
            0.0,
            EDGE_ROUNDOFF_BEGIN as f64,
            -MAX_EDGE_ROUNDOFF,
            0.0,
        );

        // Step 4: Check veininess threshold
        let veininess = toggle_value.abs();
        if veininess + edge_roundoff < VEININESS_THRESHOLD {
            return None;
        }

        // Step 5: Positional random check for solidness
        // Use the proper PositionalRandomFactory.at() matching Java's implementation
        let mut rng = self.ore_random.at(ctx.block_x, y, ctx.block_z);

        if rng.next_float() > VEIN_SOLIDNESS {
            return None;
        }

        // Step 6: Check ridged noise for vein shape
        let ridged = VEIN_RIDGED_OFFSET + vein_a.abs().max(vein_b.abs());
        if ridged >= 0.0 {
            return None;
        }

        // Step 7: Determine ore vs filler based on richness and gap
        let richness = clamp_map(
            veininess,
            VEININESS_THRESHOLD,
            MAX_RICHNESS_THRESHOLD,
            MIN_RICHNESS,
            MAX_RICHNESS,
        );

        if rng.next_float() < richness as f32 {
            // Check gap noise - simple noise lookup
            let gap = compute_vein_gap(ctx, self.noises);
            if gap > SKIP_ORE_IF_GAP_NOISE_IS_BELOW {
                // Place ore (small chance of raw ore block)
                if rng.next_float() < CHANCE_OF_RAW_ORE_BLOCK {
                    Some(vein_type.raw_ore_block())
                } else {
                    Some(vein_type.ore())
                }
            } else {
                // Gap in vein - place filler
                Some(vein_type.filler())
            }
        } else {
            // Not rich enough for ore - place filler
            Some(vein_type.filler())
        }
    }
}

// ========== Helper Functions ==========

/// Clamp and map a value from one range to another.
///
/// Equivalent to Java's Mth.clampedMap(value, fromMin, fromMax, toMin, toMax).
#[inline]
fn clamp_map(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    let clamped = value.clamp(from_min, from_max);
    let t = (clamped - from_min) / (from_max - from_min);
    to_min + t * (to_max - to_min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vein_type_y_ranges() {
        assert_eq!(VeinType::Copper.min_y(), 0);
        assert_eq!(VeinType::Copper.max_y(), 50);
        assert_eq!(VeinType::Iron.min_y(), -60);
        assert_eq!(VeinType::Iron.max_y(), -8);
    }

    #[test]
    fn test_clamp_map() {
        // At min: should return to_min
        assert!((clamp_map(0.0, 0.0, 20.0, -0.2, 0.0) - (-0.2)).abs() < 0.001);
        // At max: should return to_max
        assert!((clamp_map(20.0, 0.0, 20.0, -0.2, 0.0) - 0.0).abs() < 0.001);
        // Midpoint
        assert!((clamp_map(10.0, 0.0, 20.0, -0.2, 0.0) - (-0.1)).abs() < 0.001);
        // Below min: clamp to min
        assert!((clamp_map(-5.0, 0.0, 20.0, -0.2, 0.0) - (-0.2)).abs() < 0.001);
        // Above max: clamp to max
        assert!((clamp_map(30.0, 0.0, 20.0, -0.2, 0.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_vein_blocks() {
        // Just verify the blocks are valid (non-zero unless air)
        // The actual values depend on runtime block registry
        let copper = VeinType::Copper;
        let iron = VeinType::Iron;

        // These should be different blocks
        assert_ne!(copper.filler(), iron.filler());
        assert_ne!(copper.ore(), iron.ore());
    }
}
