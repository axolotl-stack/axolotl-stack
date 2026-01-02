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

use crate::world::chunk::blocks;
use crate::world::generator::density::{
    FunctionContext, FlatCacheGrid, ColumnContext, NoiseRegistry,
    compute_vein_toggle, compute_vein_ridged, compute_vein_gap,
};
use crate::world::generator::xoroshiro::PositionalRandomFactory;

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
/// Uses AOT-compiled density functions for maximum performance.
pub struct OreVeinifier<'a> {
    /// Noise registry for computing density functions.
    noises: &'a NoiseRegistry,
    /// FlatCacheGrid for Y-independent values.
    grid: &'a FlatCacheGrid,
    /// Positional random factory for ore generation.
    /// This matches Java's `RandomState.oreRandom()`.
    ore_random: PositionalRandomFactory,
}

impl<'a> OreVeinifier<'a> {
    /// Create a new ore veinifier using AOT-compiled density functions.
    ///
    /// # Arguments
    /// * `noises` - Noise registry for computing density functions
    /// * `grid` - FlatCacheGrid for Y-independent values
    /// * `ore_random` - Positional random factory for ore RNG (from `PositionalRandomFactory::fork_ore_random()`)
    pub fn new(
        noises: &'a NoiseRegistry,
        grid: &'a FlatCacheGrid,
        ore_random: PositionalRandomFactory,
    ) -> Self {
        Self {
            noises,
            grid,
            ore_random,
        }
    }

    /// Compute the block state at a position, if in an ore vein.
    ///
    /// Returns `Some(block_id)` if this position is part of an ore vein,
    /// or `None` if no vein block should be placed (let other fillers handle).
    pub fn compute(&self, ctx: &FunctionContext) -> Option<u32> {
        let y = ctx.block_y;

        // Create ColumnContext for this column
        let col = ColumnContext::new(ctx.block_x, ctx.block_z, self.noises, self.grid);

        // Step 1: Compute vein toggle to determine vein type
        let toggle_value = compute_vein_toggle(ctx, self.noises, self.grid, &col);

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
        let ridged = compute_vein_ridged(ctx, self.noises, self.grid, &col);
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
            // Check gap noise
            let gap = compute_vein_gap(ctx, self.noises, self.grid, &col);
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
