//! 3D grid-based aquifer system for underground water and lava pockets.
//!
//! This module implements Java Edition's aquifer system, which creates
//! underground water pockets and lava pools with realistic fluid levels
//! and pressure-based blending between adjacent aquifers.
//!
//! ## Algorithm Overview
//!
//! The aquifer system divides the world into a 3D grid:
//! - X spacing: 16 blocks
//! - Y spacing: 12 blocks
//! - Z spacing: 16 blocks
//!
//! Each grid cell contains an aquifer center with a randomized position
//! within the cell. The aquifer's fluid level and type are determined by:
//! - Floodedness noise - controls whether an aquifer exists
//! - Spread noise - varies fluid levels between aquifers
//! - Lava noise - determines lava placement in deep regions
//!
//! When computing block state, the algorithm:
//! 1. Finds the 4 nearest aquifer centers
//! 2. Calculates similarity (inverse distance relationship)
//! 3. Applies pressure-based blending at aquifer boundaries
//! 4. Returns appropriate fluid or barrier blocks

use crate::world::chunk::blocks;
use crate::world::generator::density::{DensityFunction, FunctionContext, SinglePointContext};
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::sync::Arc;

// ========== Constants ==========

/// X spacing between aquifer grid cells (blocks).
#[allow(dead_code)]
const X_SPACING: i32 = 16;
/// Y spacing between aquifer grid cells (blocks).
const Y_SPACING: i32 = 12;
/// Z spacing between aquifer grid cells (blocks).
#[allow(dead_code)]
const Z_SPACING: i32 = 16;

/// Bit shift for X spacing (16 = 2^4).
const X_SPACING_SHIFT: i32 = 4;
/// Bit shift for Z spacing (16 = 2^4).
const Z_SPACING_SHIFT: i32 = 4;

/// Range for randomizing aquifer center X position.
const X_RANGE: i32 = 10;
/// Range for randomizing aquifer center Y position.
const Y_RANGE: i32 = 9;
/// Range for randomizing aquifer center Z position.
const Z_RANGE: i32 = 10;

/// Sampling offset for grid lookup.
const SAMPLE_OFFSET_X: i32 = -5;
/// Sampling offset for grid lookup.
const SAMPLE_OFFSET_Y: i32 = 1;
/// Sampling offset for grid lookup.
const SAMPLE_OFFSET_Z: i32 = -5;

/// Y value below which lava can appear.
const LAVA_THRESHOLD_Y: i32 = -10;

/// Value indicating no aquifer exists at this position.
/// Matches Java's DimensionType.WAY_BELOW_MIN_Y.
const WAY_BELOW_MIN_Y: i32 = i32::MIN + 1;

/// Surface sampling offsets in chunk coordinates.
/// Used to find nearby surface levels for aquifer computation.
#[allow(dead_code)]
const SURFACE_SAMPLING_OFFSETS_IN_CHUNKS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

// ========== Fluid Types ==========

/// Type of fluid in an aquifer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidType {
    /// Water (default fluid).
    Water,
    /// Lava (appears in deep regions).
    Lava,
}

impl FluidType {
    /// Get the block ID for this fluid type.
    pub fn block_id(&self) -> u32 {
        match self {
            FluidType::Water => *blocks::WATER,
            FluidType::Lava => *blocks::LAVA,
        }
    }
}

// ========== Fluid Status ==========

/// Fluid status at an aquifer center.
///
/// Contains the fluid level (Y coordinate) and fluid type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidStatus {
    /// Y coordinate of the fluid surface.
    pub fluid_level: i32,
    /// Type of fluid (water or lava).
    pub fluid_type: FluidType,
}

impl FluidStatus {
    /// Create a new fluid status.
    pub fn new(fluid_level: i32, fluid_type: FluidType) -> Self {
        Self {
            fluid_level,
            fluid_type,
        }
    }

    /// Get the block at the given Y coordinate.
    ///
    /// Returns the fluid block if below the fluid level, otherwise air.
    pub fn at(&self, y: i32) -> u32 {
        if y < self.fluid_level {
            self.fluid_type.block_id()
        } else {
            *blocks::AIR
        }
    }
}

// ========== Grid Coordinate Conversion ==========

/// Convert world X coordinate to grid X coordinate.
#[inline]
pub fn grid_x(x: i32) -> i32 {
    x >> X_SPACING_SHIFT
}

/// Convert grid X coordinate to world X coordinate.
#[inline]
pub fn from_grid_x(grid_x: i32, offset: i32) -> i32 {
    (grid_x << X_SPACING_SHIFT) + offset
}

/// Convert world Y coordinate to grid Y coordinate.
#[inline]
pub fn grid_y(y: i32) -> i32 {
    y.div_euclid(Y_SPACING)
}

/// Convert grid Y coordinate to world Y coordinate.
#[inline]
pub fn from_grid_y(grid_y: i32, offset: i32) -> i32 {
    grid_y * Y_SPACING + offset
}

/// Convert world Z coordinate to grid Z coordinate.
#[inline]
pub fn grid_z(z: i32) -> i32 {
    z >> Z_SPACING_SHIFT
}

/// Convert grid Z coordinate to world Z coordinate.
#[inline]
pub fn from_grid_z(grid_z: i32, offset: i32) -> i32 {
    (grid_z << Z_SPACING_SHIFT) + offset
}

/// Pack block coordinates into a single i64.
#[inline]
fn pack_pos(x: i32, y: i32, z: i32) -> i64 {
    // Pack as: x in bits 38-63, z in bits 12-37, y in bits 0-11
    // This matches Java's BlockPos.asLong() format
    let x = x as i64 & 0x3FFFFFF; // 26 bits
    let y = y as i64 & 0xFFF; // 12 bits
    let z = z as i64 & 0x3FFFFFF; // 26 bits
    (x << 38) | (z << 12) | y
}

/// Unpack X coordinate from packed position.
#[inline]
fn unpack_x(packed: i64) -> i32 {
    (packed >> 38) as i32
}

/// Unpack Y coordinate from packed position.
#[inline]
fn unpack_y(packed: i64) -> i32 {
    ((packed << 52) >> 52) as i32 // Sign extend from 12 bits
}

/// Unpack Z coordinate from packed position.
#[inline]
fn unpack_z(packed: i64) -> i32 {
    ((packed << 26) >> 38) as i32 // Sign extend from 26 bits
}

// ========== Fluid Picker ==========

/// Global fluid picker for determining base fluid levels.
pub trait FluidPicker: Send + Sync {
    /// Compute the global fluid status at the given position.
    fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus;
}

/// Default overworld fluid picker.
///
/// Returns water at sea level for most positions.
pub struct OverworldFluidPicker {
    /// Sea level (default water level).
    pub sea_level: i32,
}

impl OverworldFluidPicker {
    /// Create a new overworld fluid picker.
    pub fn new(sea_level: i32) -> Self {
        Self { sea_level }
    }
}

impl FluidPicker for OverworldFluidPicker {
    fn compute_fluid(&self, _x: i32, y: i32, _z: i32) -> FluidStatus {
        // Below Y=-54, use lava
        if y < -54 {
            FluidStatus::new(-54, FluidType::Lava)
        } else {
            FluidStatus::new(self.sea_level, FluidType::Water)
        }
    }
}

// ========== Noise-Based Aquifer ==========

/// 3D noise-based aquifer system.
///
/// This is the main aquifer implementation that creates underground
/// water and lava pockets with pressure-based blending.
#[allow(dead_code)]
pub struct NoiseBasedAquifer {
    // Noise functions from router
    barrier_noise: Arc<dyn DensityFunction>,
    floodedness_noise: Arc<dyn DensityFunction>,
    spread_noise: Arc<dyn DensityFunction>,
    lava_noise: Arc<dyn DensityFunction>,
    /// Erosion function (used for deep dark region detection - future use).
    erosion: Arc<dyn DensityFunction>,
    /// Depth function (used for deep dark region detection - future use).
    depth: Arc<dyn DensityFunction>,

    // World seed for RNG
    seed: i64,

    // Grid bounds
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_y: i32,
    grid_size_z: i32,

    // Caches
    aquifer_cache: Vec<Option<FluidStatus>>,
    location_cache: Vec<i64>,

    // Global fluid picker
    global_fluid_picker: Box<dyn FluidPicker>,

    // State
    /// Whether a fluid update should be scheduled.
    pub should_schedule_fluid_update: bool,

    /// Skip sampling above this Y level (optimization).
    skip_sampling_above_y: i32,
}

impl NoiseBasedAquifer {
    /// Create a new noise-based aquifer.
    ///
    /// # Arguments
    /// * `chunk_x` - Chunk X coordinate
    /// * `chunk_z` - Chunk Z coordinate
    /// * `min_y` - Minimum Y coordinate
    /// * `height` - World height
    /// * `barrier_noise` - Barrier noise function from router
    /// * `floodedness_noise` - Floodedness noise function from router
    /// * `spread_noise` - Spread noise function from router
    /// * `lava_noise` - Lava noise function from router
    /// * `erosion` - Erosion function from router
    /// * `depth` - Depth function from router
    /// * `seed` - World seed
    /// * `fluid_picker` - Global fluid picker
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        min_y: i32,
        height: i32,
        barrier_noise: Arc<dyn DensityFunction>,
        floodedness_noise: Arc<dyn DensityFunction>,
        spread_noise: Arc<dyn DensityFunction>,
        lava_noise: Arc<dyn DensityFunction>,
        erosion: Arc<dyn DensityFunction>,
        depth: Arc<dyn DensityFunction>,
        seed: i64,
        fluid_picker: Box<dyn FluidPicker>,
    ) -> Self {
        let min_block_x = chunk_x * 16;
        let max_block_x = min_block_x + 15;
        let min_block_z = chunk_z * 16;
        let max_block_z = min_block_z + 15;

        // Calculate grid bounds (with margin for neighbor sampling)
        let min_grid_x = grid_x(min_block_x + SAMPLE_OFFSET_X);
        let max_grid_x = grid_x(max_block_x + SAMPLE_OFFSET_X) + 1;
        let grid_size_x = max_grid_x - min_grid_x + 1;

        let min_grid_y = grid_y(min_y + SAMPLE_OFFSET_Y) - 1;
        let max_grid_y = grid_y(min_y + height + SAMPLE_OFFSET_Y) + 1;
        let grid_size_y = max_grid_y - min_grid_y + 1;

        let min_grid_z = grid_z(min_block_z + SAMPLE_OFFSET_Z);
        let max_grid_z = grid_z(max_block_z + SAMPLE_OFFSET_Z) + 1;
        let grid_size_z = max_grid_z - min_grid_z + 1;

        let cache_size = (grid_size_x * grid_size_y * grid_size_z) as usize;

        Self {
            barrier_noise,
            floodedness_noise,
            spread_noise,
            lava_noise,
            erosion,
            depth,
            seed,
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_y,
            grid_size_z,
            aquifer_cache: vec![None; cache_size],
            location_cache: vec![i64::MAX; cache_size],
            global_fluid_picker: fluid_picker,
            should_schedule_fluid_update: false,
            skip_sampling_above_y: 256, // Will be computed from surface level
        }
    }

    /// Get cache index from grid coordinates.
    fn get_index(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> usize {
        let x = grid_x - self.min_grid_x;
        let y = grid_y - self.min_grid_y;
        let z = grid_z - self.min_grid_z;
        ((y * self.grid_size_z + z) * self.grid_size_x + x) as usize
    }

    /// Compute block state considering aquifer.
    ///
    /// Returns `Some(block_id)` if the aquifer system determines the block,
    /// or `None` if the density should be used normally (solid block).
    pub fn compute_substance(&mut self, ctx: &dyn FunctionContext, density: f64) -> Option<u32> {
        // Positive density = solid block, let normal generation handle it
        if density > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        let x = ctx.block_x();
        let y = ctx.block_y();
        let z = ctx.block_z();

        // Check global fluid first
        let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);

        // Above skip threshold, just use global fluid
        if y > self.skip_sampling_above_y {
            self.should_schedule_fluid_update = false;
            return Some(global_fluid.at(y));
        }

        // If global fluid is lava, return it directly
        if global_fluid.at(y) == *blocks::LAVA {
            self.should_schedule_fluid_update = false;
            return Some(*blocks::LAVA);
        }

        // Find the 4 nearest aquifer centers
        let gx = grid_x(x + SAMPLE_OFFSET_X);
        let gy = grid_y(y + SAMPLE_OFFSET_Y);
        let gz = grid_z(z + SAMPLE_OFFSET_Z);

        // Track 4 closest aquifers
        let mut closest_indices = [0usize; 4];
        let mut closest_distances = [i32::MAX; 4];

        // Search 2x3x2 grid cells
        for dx in 0..=1 {
            for dy in -1..=1 {
                for dz in 0..=1 {
                    let cell_x = gx + dx;
                    let cell_y = gy + dy;
                    let cell_z = gz + dz;

                    let index = self.get_index(cell_x, cell_y, cell_z);
                    if index >= self.location_cache.len() {
                        continue;
                    }

                    // Get or compute aquifer location
                    let pos = if self.location_cache[index] != i64::MAX {
                        self.location_cache[index]
                    } else {
                        let pos = self.compute_aquifer_location(cell_x, cell_y, cell_z);
                        self.location_cache[index] = pos;
                        pos
                    };

                    // Calculate distance squared
                    let ax = unpack_x(pos) - x;
                    let ay = unpack_y(pos) - y;
                    let az = unpack_z(pos) - z;
                    let dist_sq = ax * ax + ay * ay + az * az;

                    // Insert into sorted list of 4 closest
                    self.insert_sorted(
                        &mut closest_indices,
                        &mut closest_distances,
                        index,
                        dist_sq,
                    );
                }
            }
        }

        // Get fluid status of closest aquifer
        let fluid1 = self.get_aquifer_status(closest_indices[0]);
        let block_at_y = fluid1.at(y);

        // Calculate similarity between distances
        let similarity = self.similarity(closest_distances[0], closest_distances[1]);

        if similarity <= 0.0 {
            // Only closest aquifer matters
            self.should_schedule_fluid_update = false;
            return Some(block_at_y);
        }

        // Check if water meets lava below
        if block_at_y == *blocks::WATER {
            let below_fluid = self.global_fluid_picker.compute_fluid(x, y - 1, z);
            if below_fluid.at(y - 1) == *blocks::LAVA {
                self.should_schedule_fluid_update = true;
                return Some(block_at_y);
            }
        }

        // Calculate pressure for blending
        let fluid2 = self.get_aquifer_status(closest_indices[1]);
        let mut barrier_value = f64::NAN;
        let pressure1 = similarity * self.calculate_pressure(ctx, &mut barrier_value, &fluid1, &fluid2);

        if density + pressure1 > 0.0 {
            self.should_schedule_fluid_update = false;
            return None; // Barrier - solid block
        }

        // Check third aquifer
        let fluid3 = self.get_aquifer_status(closest_indices[2]);
        let similarity2 = self.similarity(closest_distances[0], closest_distances[2]);
        if similarity2 > 0.0 {
            let pressure2 = similarity * similarity2 * self.calculate_pressure(ctx, &mut barrier_value, &fluid1, &fluid3);
            if density + pressure2 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // Check blending between second and third
        let similarity3 = self.similarity(closest_distances[1], closest_distances[2]);
        if similarity3 > 0.0 {
            let pressure3 = similarity * similarity3 * self.calculate_pressure(ctx, &mut barrier_value, &fluid2, &fluid3);
            if density + pressure3 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // Determine if fluid update is needed
        let flowing_similarity = self.similarity(100, 144); // Constants from Java
        let different_12 = fluid1 != fluid2;
        let different_23_sim = similarity3 >= flowing_similarity && fluid2 != fluid3;
        let different_13_sim = similarity2 >= flowing_similarity && fluid1 != fluid3;

        self.should_schedule_fluid_update = different_12 || different_23_sim || different_13_sim;

        Some(block_at_y)
    }

    /// Compute the randomized location of an aquifer center.
    fn compute_aquifer_location(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> i64 {
        // Create positional RNG
        let seed_mix = self.seed
            .wrapping_add(grid_x as i64 * 341873128712)
            .wrapping_add(grid_y as i64 * 132897987541)
            .wrapping_add(grid_z as i64 * 1664525);
        let mut rng = Xoroshiro128::from_seed(seed_mix);

        let offset_x = rng.next_int(X_RANGE as u32) as i32;
        let offset_y = rng.next_int(Y_RANGE as u32) as i32;
        let offset_z = rng.next_int(Z_RANGE as u32) as i32;

        let x = from_grid_x(grid_x, offset_x);
        let y = from_grid_y(grid_y, offset_y);
        let z = from_grid_z(grid_z, offset_z);

        pack_pos(x, y, z)
    }

    /// Get fluid status for an aquifer by cache index.
    fn get_aquifer_status(&mut self, index: usize) -> FluidStatus {
        if let Some(status) = self.aquifer_cache[index] {
            return status;
        }

        let pos = self.location_cache[index];
        let status = self.compute_fluid(unpack_x(pos), unpack_y(pos), unpack_z(pos));
        self.aquifer_cache[index] = Some(status);
        status
    }

    /// Compute fluid status at a position.
    ///
    /// This determines whether an aquifer exists at this position and what
    /// fluid level it should have. The fix uses sea level (63) as the base
    /// instead of the problematic grid-based calculation that caused water pillars.
    fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus {
        let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);
        let ctx = SinglePointContext::new(x, y, z);

        // Sample floodedness noise
        let floodedness = self.floodedness_noise.compute(&ctx).clamp(-1.0, 1.0);

        // Fully flooded = use global sea level
        if floodedness > 0.8 {
            return global_fluid;
        }

        // Partially flooded = compute level based on position but relative to sea level
        if floodedness > 0.3 {
            let spread = self.spread_noise.compute(&ctx) * 10.0;
            let quantized = (spread / 3.0).round() as i32 * 3;
            // Use sea level as base, not arbitrary grid position
            // This prevents water pillars appearing at random heights
            const SEA_LEVEL: i32 = 63;
            let variation = quantized.clamp(-20, 20); // Limit variation to prevent extreme levels
            let level = SEA_LEVEL + variation;
            return FluidStatus::new(level, self.compute_fluid_type(x, y, z, &global_fluid, level));
        }

        // Not flooded - no aquifer here
        FluidStatus::new(WAY_BELOW_MIN_Y, FluidType::Water)
    }

    /// Determine fluid type (water or lava) at a position.
    fn compute_fluid_type(&self, x: i32, y: i32, z: i32, global_fluid: &FluidStatus, fluid_level: i32) -> FluidType {
        // If global fluid is already lava, use it
        if global_fluid.fluid_type == FluidType::Lava {
            return FluidType::Lava;
        }

        // Check for lava in deep regions
        if fluid_level <= LAVA_THRESHOLD_Y && fluid_level != WAY_BELOW_MIN_Y {
            // Use coarser grid for lava noise
            let lx = x.div_euclid(64);
            let ly = y.div_euclid(40);
            let lz = z.div_euclid(64);

            let ctx = SinglePointContext::new(lx, ly, lz);
            let lava_value = self.lava_noise.compute(&ctx);

            if lava_value.abs() > 0.3 {
                return FluidType::Lava;
            }
        }

        FluidType::Water
    }

    /// Calculate similarity between two squared distances.
    ///
    /// Returns a value in [0, 1] where 1 means the distances are equal
    /// and 0 means they differ by 25 or more.
    fn similarity(&self, dist1_sq: i32, dist2_sq: i32) -> f64 {
        1.0 - (dist2_sq - dist1_sq) as f64 / 25.0
    }

    /// Calculate pressure between two aquifers.
    fn calculate_pressure(
        &self,
        ctx: &dyn FunctionContext,
        barrier_value: &mut f64,
        fluid1: &FluidStatus,
        fluid2: &FluidStatus,
    ) -> f64 {
        let y = ctx.block_y();
        let block1 = fluid1.at(y);
        let block2 = fluid2.at(y);

        // Lava/water interface creates strong barrier
        if (block1 == *blocks::LAVA && block2 == *blocks::WATER)
            || (block1 == *blocks::WATER && block2 == *blocks::LAVA)
        {
            return 2.0;
        }

        // Calculate level difference
        // Use wrapping arithmetic to prevent overflow when dealing with WAY_BELOW_MIN_Y marker values
        let diff = fluid1.fluid_level.wrapping_sub(fluid2.fluid_level);
        let level_diff = diff.wrapping_abs();
        if level_diff == 0 {
            return 0.0;
        }

        // Calculate pressure based on Y position relative to fluid levels
        let mid_level = (fluid1.fluid_level + fluid2.fluid_level) as f64 / 2.0;
        let dist_from_mid = ctx.block_y() as f64 + 0.5 - mid_level;
        let half_range = level_diff as f64 / 2.0;
        let offset = half_range - dist_from_mid.abs();

        let base_pressure = if dist_from_mid > 0.0 {
            // Above midpoint
            if offset > 0.0 { offset / 1.5 } else { offset / 2.5 }
        } else {
            // Below midpoint
            let p = 3.0 + offset;
            if p > 0.0 { p / 3.0 } else { p / 10.0 }
        };

        // Add barrier noise when near boundary
        let barrier = if (-2.0..=2.0).contains(&base_pressure) {
            if barrier_value.is_nan() {
                *barrier_value = self.barrier_noise.compute(ctx);
            }
            *barrier_value
        } else {
            0.0
        };

        2.0 * (barrier + base_pressure)
    }

    /// Insert into sorted arrays of 4 closest items.
    fn insert_sorted(
        &self,
        indices: &mut [usize; 4],
        distances: &mut [i32; 4],
        new_index: usize,
        new_dist: i32,
    ) {
        if new_dist >= distances[3] {
            return; // Not closer than any of the 4
        }

        // Find insertion position
        let pos = if new_dist < distances[0] {
            0
        } else if new_dist < distances[1] {
            1
        } else if new_dist < distances[2] {
            2
        } else {
            3
        };

        // Shift elements down
        for i in (pos + 1..4).rev() {
            indices[i] = indices[i - 1];
            distances[i] = distances[i - 1];
        }

        // Insert new element
        indices[pos] = new_index;
        distances[pos] = new_dist;
    }
}

// ========== Disabled Aquifer ==========

/// Disabled aquifer that just uses global fluid picker.
///
/// Used for dimensions or generation modes that don't need aquifers.
pub struct DisabledAquifer {
    fluid_picker: Box<dyn FluidPicker>,
}

impl DisabledAquifer {
    /// Create a new disabled aquifer.
    pub fn new(fluid_picker: Box<dyn FluidPicker>) -> Self {
        Self { fluid_picker }
    }

    /// Compute substance without aquifer logic.
    pub fn compute_substance(&self, ctx: &dyn FunctionContext, density: f64) -> Option<u32> {
        if density > 0.0 {
            None // Solid block
        } else {
            let y = ctx.block_y();
            Some(
                self.fluid_picker
                    .compute_fluid(ctx.block_x(), y, ctx.block_z())
                    .at(y),
            )
        }
    }

    /// Disabled aquifer never schedules fluid updates.
    pub fn should_schedule_fluid_update(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_x() {
        assert_eq!(grid_x(0), 0);
        assert_eq!(grid_x(15), 0);
        assert_eq!(grid_x(16), 1);
        assert_eq!(grid_x(31), 1);
        assert_eq!(grid_x(32), 2);
        assert_eq!(grid_x(-1), -1);
        assert_eq!(grid_x(-16), -1);
        assert_eq!(grid_x(-17), -2);
    }

    #[test]
    fn test_grid_y() {
        assert_eq!(grid_y(0), 0);
        assert_eq!(grid_y(11), 0);
        assert_eq!(grid_y(12), 1);
        assert_eq!(grid_y(23), 1);
        assert_eq!(grid_y(24), 2);
        assert_eq!(grid_y(-1), -1);
        assert_eq!(grid_y(-12), -1);
        assert_eq!(grid_y(-13), -2);
    }

    #[test]
    fn test_grid_z() {
        assert_eq!(grid_z(0), 0);
        assert_eq!(grid_z(16), 1);
        assert_eq!(grid_z(-1), -1);
        assert_eq!(grid_z(-16), -1);
    }

    #[test]
    fn test_from_grid_coords() {
        assert_eq!(from_grid_x(0, 0), 0);
        assert_eq!(from_grid_x(1, 0), 16);
        assert_eq!(from_grid_x(1, 5), 21);

        assert_eq!(from_grid_y(0, 0), 0);
        assert_eq!(from_grid_y(1, 0), 12);
        assert_eq!(from_grid_y(2, 5), 29);

        assert_eq!(from_grid_z(0, 0), 0);
        assert_eq!(from_grid_z(1, 0), 16);
    }

    #[test]
    fn test_pack_unpack_pos() {
        let test_cases = [
            (0, 0, 0),
            (100, 64, 200),
            (-100, -50, -200),
            (1000000, 100, 1000000),
        ];

        for (x, y, z) in test_cases {
            let packed = pack_pos(x, y, z);
            assert_eq!(unpack_x(packed), x, "X mismatch for ({}, {}, {})", x, y, z);
            // Y is 12-bit signed, so only low bits matter
            let expected_y = ((y as i32) << 20) >> 20; // Sign extend 12 bits
            assert_eq!(
                unpack_y(packed),
                expected_y,
                "Y mismatch for ({}, {}, {})",
                x,
                y,
                z
            );
            assert_eq!(unpack_z(packed), z, "Z mismatch for ({}, {}, {})", x, y, z);
        }
    }

    #[test]
    fn test_fluid_status() {
        let water_status = FluidStatus::new(63, FluidType::Water);
        assert_eq!(water_status.at(62), *blocks::WATER);
        assert_eq!(water_status.at(63), *blocks::AIR);
        assert_eq!(water_status.at(64), *blocks::AIR);

        let lava_status = FluidStatus::new(-10, FluidType::Lava);
        assert_eq!(lava_status.at(-11), *blocks::LAVA);
        assert_eq!(lava_status.at(-10), *blocks::AIR);
    }

    #[test]
    fn test_overworld_fluid_picker() {
        let picker = OverworldFluidPicker::new(63);

        // Above lava threshold
        let status = picker.compute_fluid(0, 0, 0);
        assert_eq!(status.fluid_level, 63);
        assert_eq!(status.fluid_type, FluidType::Water);

        // Deep underground
        let status = picker.compute_fluid(0, -60, 0);
        assert_eq!(status.fluid_level, -54);
        assert_eq!(status.fluid_type, FluidType::Lava);
    }

    #[test]
    fn test_disabled_aquifer() {
        let picker = Box::new(OverworldFluidPicker::new(63));
        let aquifer = DisabledAquifer::new(picker);

        let ctx = SinglePointContext::new(0, 62, 0);

        // Below sea level with negative density = water
        let result = aquifer.compute_substance(&ctx, -1.0);
        assert_eq!(result, Some(*blocks::WATER));

        // Positive density = None (solid)
        let result = aquifer.compute_substance(&ctx, 1.0);
        assert_eq!(result, None);

        // Above sea level with negative density = air
        let ctx_above = SinglePointContext::new(0, 64, 0);
        let result = aquifer.compute_substance(&ctx_above, -1.0);
        assert_eq!(result, Some(*blocks::AIR));

        assert!(!aquifer.should_schedule_fluid_update());
    }
}
