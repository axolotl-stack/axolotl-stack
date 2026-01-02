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
use crate::world::generator::density::{
    FunctionContext, FlatCacheGrid, ColumnContext, NoiseRegistry,
    compute_barrier, compute_erosion, compute_depth,
    compute_fluid_level_floodedness, compute_fluid_level_spread, compute_lava,
    compute_preliminary_surface_level,
};
use crate::world::generator::xoroshiro::PositionalRandomFactory;

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

/// Erosion threshold for deep dark region detection.
/// Java: OverworldBiomeBuilder.EROSION_DEEP_DARK_DRYNESS_THRESHOLD = -0.225F
const EROSION_DEEP_DARK_THRESHOLD: f64 = -0.225;

/// Depth threshold for deep dark region detection.
/// Java: OverworldBiomeBuilder.DEPTH_DEEP_DARK_DRYNESS_THRESHOLD = 0.9F
const DEPTH_DEEP_DARK_THRESHOLD: f64 = 0.9;

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

// ========== Deep Dark Region Check ==========

/// Check if position is in a deep dark region where aquifers are disabled.
///
/// Java: OverworldBiomeBuilder.isDeepDarkRegion()
/// Deep dark regions have erosion < -0.225 AND depth > 0.9
#[inline]
fn is_deep_dark_region(
    ctx: &FunctionContext,
    noises: &NoiseRegistry,
    grid: &FlatCacheGrid,
    col: &ColumnContext,
) -> bool {
    compute_erosion(ctx, noises, grid, col) < EROSION_DEEP_DARK_THRESHOLD
        && compute_depth(ctx, noises, grid, col) > DEPTH_DEEP_DARK_THRESHOLD
}

// ========== Noise-Based Aquifer ==========

/// 3D noise-based aquifer system.
///
/// This is the main aquifer implementation that creates underground
/// water and lava pockets with pressure-based blending.
/// Uses AOT-compiled density functions for maximum performance.
pub struct NoiseBasedAquifer<'a> {
    // Noise registry for computing density functions
    noises: &'a NoiseRegistry,

    // FlatCacheGrid for Y-independent values
    grid: &'a FlatCacheGrid,

    // Positional random factory for aquifer center locations
    positional_random: PositionalRandomFactory,

    // Chunk block bounds (for FlatCacheGrid bounds checking)
    min_block_x: i32,
    max_block_x: i32,
    min_block_z: i32,
    max_block_z: i32,

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
    /// Computed from preliminary surface level at chunk initialization.
    skip_sampling_above_y: i32,

    // Per-column ColumnContext cache - avoids recreating expensive ColumnContext per block
    // We cache up to 4 contexts (for 4 Z positions in a cell) with the same X coordinate.
    // Key: (block_x, base_block_z), Value: array of 4 cached ColumnContexts for z, z+1, z+2, z+3
    cached_column_ctx_x: i32,
    cached_column_ctx_base_z: i32,
    cached_column_ctx: [ColumnContext; 4],
    cached_column_ctx_valid: bool,
}

impl<'a> NoiseBasedAquifer<'a> {
    /// Create a new noise-based aquifer using AOT-compiled density functions.
    ///
    /// # Arguments
    /// * `chunk_x` - Chunk X coordinate
    /// * `chunk_z` - Chunk Z coordinate
    /// * `min_y` - Minimum Y coordinate
    /// * `height` - World height
    /// * `noises` - Noise registry for computing density functions
    /// * `grid` - FlatCacheGrid for Y-independent values
    /// * `seed` - World seed
    /// * `fluid_picker` - Global fluid picker
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        min_y: i32,
        height: i32,
        noises: &'a NoiseRegistry,
        grid: &'a FlatCacheGrid,
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

        // Compute max preliminary surface level for skip optimization
        // Java: this.adjustSurfaceLevel(noiseChunk.maxPreliminarySurfaceLevel(...))
        // Sample the 4 corners of the grid to find max surface
        let max_surface = Self::compute_max_preliminary_surface(
            noises,
            grid,
            from_grid_x(min_grid_x, 0),
            from_grid_z(min_grid_z, 0),
            from_grid_x(max_grid_x, X_RANGE - 1),
            from_grid_z(max_grid_z, Z_RANGE - 1),
        );
        // Java: int p = this.adjustSurfaceLevel(maxPreliminarySurfaceLevel) = max_surface + 8
        // Java: int q = gridY(p + 12) - -1 = gridY(p + 12) + 1
        // Java: this.skipSamplingAboveY = fromGridY(q, 11) - 1
        // Note: Java uses 11 (Y_SPACING - 1 = 12 - 1) as offset to get max Y in cell
        let adjusted_surface = max_surface + 8;  // Java: adjustSurfaceLevel()
        let skip_grid_y = grid_y(adjusted_surface + 12) + 1;  // Java: gridY(p + 12) - -1
        let skip_sampling_above_y = from_grid_y(skip_grid_y, Y_SPACING - 1) - 1;  // Java: fromGridY(q, 11) - 1

        // Create positional random factory from seed
        // Java: this.positionalRandomFactory = positionalRandomFactory (passed from RandomState)
        // Java derives aquifer random via: this.random.fromHashOf("minecraft:aquifer").forkPositional()
        // where this.random is already a PositionalRandomFactory from world seed
        let base_random = PositionalRandomFactory::new(seed);
        let positional_random = base_random.fork_aquifer_random();

        Self {
            noises,
            grid,
            positional_random,
            min_block_x,
            max_block_x,
            min_block_z,
            max_block_z,
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
            skip_sampling_above_y,
            cached_column_ctx_x: i32::MIN,
            cached_column_ctx_base_z: i32::MIN,
            cached_column_ctx: [ColumnContext::default(); 4],
            cached_column_ctx_valid: false,
        }
    }

    /// Get or compute ColumnContext for the given (x, z) position.
    /// Uses a cache that holds 4 consecutive Z positions (for cell processing).
    /// This is optimized for the terrain loop which processes z_in_cell = 0..4.
    ///
    /// IMPORTANT: Positions outside the chunk bounds use standalone mode to avoid
    /// FlatCacheGrid out-of-bounds access.
    #[inline]
    fn get_column_context(&mut self, x: i32, z: i32) -> ColumnContext {
        // Check if this position is in the cached batch
        if self.cached_column_ctx_valid {
            let z_offset = z - self.cached_column_ctx_base_z;
            if x == self.cached_column_ctx_x && z_offset >= 0 && z_offset < 4 {
                return self.cached_column_ctx[z_offset as usize];
            }
        }

        // Cache miss - compute new batch of 4 contexts
        // We assume terrain loop processes z_in_cell = 0..4, so we cache z, z+1, z+2, z+3
        // But if z is not aligned to 4, we use z as base
        let base_z = z & !3; // Align to 4 (e.g., z=5 -> base_z=4)

        // Check if positions are within chunk bounds for FlatCacheGrid access
        // FlatCacheGrid is only valid for positions within [min_block_x, max_block_x] x [min_block_z, max_block_z]
        let x_in_bounds = x >= self.min_block_x && x <= self.max_block_x;

        for i in 0..4 {
            let cur_z = base_z + i;
            let z_in_bounds = cur_z >= self.min_block_z && cur_z <= self.max_block_z;

            // For positions within the chunk bounds, use the cached grid.
            // For out-of-bounds, create a new FlatCacheGrid for that position.
            self.cached_column_ctx[i as usize] = if x_in_bounds && z_in_bounds {
                ColumnContext::new(x, cur_z, self.noises, self.grid)
            } else {
                // Out of bounds - create FlatCacheGrid for the chunk containing this position
                let chunk_x = x >> 4;
                let chunk_z = cur_z >> 4;
                let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
                ColumnContext::new(x, cur_z, self.noises, &grid)
            };
        }

        self.cached_column_ctx_x = x;
        self.cached_column_ctx_base_z = base_z;
        self.cached_column_ctx_valid = true;

        let z_offset = z - base_z;
        self.cached_column_ctx[z_offset as usize]
    }

    /// Compute max preliminary surface level over a region using AOT function.
    ///
    /// This samples positions that may be outside the current chunk, so we create
    /// FlatCacheGrids for each sample position.
    ///
    /// Java reference: NoiseChunk.maxPreliminarySurfaceLevel()
    /// Java iterates from min to max in steps of 4 (quart size).
    fn compute_max_preliminary_surface(
        noises: &NoiseRegistry,
        _grid: &FlatCacheGrid,
        min_x: i32,
        min_z: i32,
        max_x: i32,
        max_z: i32,
    ) -> i32 {
        let mut max_surface: i32 = i32::MIN;

        // Java: for (int n = j; n <= l; n += 4) { for (int o = i; o <= k; o += 4) { ... } }
        // Sample the entire grid region stepping by 4 (quart size)
        let mut z = min_z;
        while z <= max_z {
            let mut x = min_x;
            while x <= max_x {
                // Java: preliminarySurfaceLevel quantizes to quart boundaries
                // QuartPos.toBlock(QuartPos.fromBlock(x)) = (x >> 2) << 2
                let quart_x = (x >> 2) << 2;
                let quart_z = (z >> 2) << 2;

                let ctx = FunctionContext::new(quart_x, 0, quart_z);

                // Create FlatCacheGrid for the chunk containing this sample position
                let chunk_x = quart_x >> 4;
                let chunk_z = quart_z >> 4;
                let sample_grid = FlatCacheGrid::new(chunk_x, chunk_z, noises);
                let col = ColumnContext::new(quart_x, quart_z, noises, &sample_grid);
                let surface = compute_preliminary_surface_level(&ctx, noises, &sample_grid, &col).floor() as i32;
                max_surface = max_surface.max(surface);

                x += 4;
            }
            z += 4;
        }
        max_surface
    }

    /// Get cache index from grid coordinates.
    /// Returns usize::MAX if coordinates are out of bounds.
    fn get_index(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> usize {
        let x = grid_x - self.min_grid_x;
        let y = grid_y - self.min_grid_y;
        let z = grid_z - self.min_grid_z;

        // Bounds check before computing index
        if x < 0 || y < 0 || z < 0
            || x >= self.grid_size_x
            || y >= self.grid_size_y
            || z >= self.grid_size_z
        {
            return usize::MAX;
        }

        ((y * self.grid_size_z + z) * self.grid_size_x + x) as usize
    }

    /// Compute block state considering aquifer.
    ///
    /// Returns `Some(block_id)` if the aquifer system determines the block,
    /// or `None` if the density should be used normally (solid block).
    pub fn compute_substance(&mut self, ctx: &FunctionContext, density: f64) -> Option<u32> {
        // Positive density = solid block, let normal generation handle it
        if density > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        let x = ctx.block_x;
        let y = ctx.block_y;
        let z = ctx.block_z;

        // Get cached ColumnContext for this column - used by calculate_pressure for barrier noise.
        // This is a critical optimization: ColumnContext::new() is very expensive (lots of noise
        // sampling), and we call compute_substance for every non-solid block. Caching avoids
        // recreating the context when processing multiple Y positions in the same column.
        let col = self.get_column_context(x, z);

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
                    if index == usize::MAX {
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
        // Java: BlockState blockState = fluidStatus2.at(j);
        // When aquifer returns WAY_BELOW_MIN_Y (no flooding), fluid1.at(y) returns AIR.
        // This is CORRECT - caves should NOT be flooded by global fluid.
        // Ocean water comes from the early return when y > skipSamplingAboveY.
        let block_at_y = fluid1.at(y);

        // Calculate similarity between distances
        let similarity = self.similarity(closest_distances[0], closest_distances[1]);

        // FLOWING_UPDATE_SIMILARITY = similarity(100, 144) = 1.0 - 44/25 = -0.76
        // Java: if (e <= 0.0) { if (e >= FLOWING) { check fluid } else { no update } return; }
        let flowing_update_similarity = self.similarity(100, 144);

        if similarity <= 0.0 {
            // Only closest aquifer matters for block placement
            // But check if we need to schedule fluid update
            if similarity >= flowing_update_similarity {
                let fluid2 = self.get_aquifer_status(closest_indices[1]);
                self.should_schedule_fluid_update = fluid1 != fluid2;
            } else {
                self.should_schedule_fluid_update = false;
            }
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
        let pressure1 = similarity * self.calculate_pressure(ctx, &col, &mut barrier_value, &fluid1, &fluid2);

        if density + pressure1 > 0.0 {
            self.should_schedule_fluid_update = false;
            return None; // Barrier - solid block
        }

        // Check third aquifer
        let fluid3 = self.get_aquifer_status(closest_indices[2]);
        let similarity2 = self.similarity(closest_distances[0], closest_distances[2]);
        if similarity2 > 0.0 {
            let pressure2 = similarity * similarity2 * self.calculate_pressure(ctx, &col, &mut barrier_value, &fluid1, &fluid3);
            if density + pressure2 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // Check blending between second and third
        let similarity3 = self.similarity(closest_distances[1], closest_distances[2]);
        if similarity3 > 0.0 {
            let pressure3 = similarity * similarity3 * self.calculate_pressure(ctx, &col, &mut barrier_value, &fluid2, &fluid3);
            if density + pressure3 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // Determine if fluid update is needed
        // Java logic (lines 275-284):
        // boolean bl = !fluid1.equals(fluid2);
        // boolean bl2 = h >= FLOWING && !fluid2.equals(fluid3);
        // boolean bl3 = g >= FLOWING && !fluid1.equals(fluid3);
        // if (!bl && !bl2 && !bl3) {
        //     // Only then check 4th aquifer
        //     shouldUpdate = g >= FLOWING && similarity(dist1, dist4) >= FLOWING && !fluid1.equals(fluid4);
        // } else {
        //     shouldUpdate = true;
        // }
        let different_12 = fluid1 != fluid2;
        let different_23_sim = similarity3 >= flowing_update_similarity && fluid2 != fluid3;
        let different_13_sim = similarity2 >= flowing_update_similarity && fluid1 != fluid3;

        if !different_12 && !different_23_sim && !different_13_sim {
            // Check 4th aquifer
            let similarity4 = self.similarity(closest_distances[0], closest_distances[3]);
            if similarity2 >= flowing_update_similarity && similarity4 >= flowing_update_similarity {
                let fluid4 = self.get_aquifer_status(closest_indices[3]);
                self.should_schedule_fluid_update = fluid1 != fluid4;
            } else {
                self.should_schedule_fluid_update = false;
            }
        } else {
            self.should_schedule_fluid_update = true;
        }

        Some(block_at_y)
    }

    /// Compute the randomized location of an aquifer center.
    /// Uses the positional random factory to match Java's exact RNG behavior.
    fn compute_aquifer_location(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> i64 {
        // Java: RandomSource randomSource = this.positionalRandomFactory.at(z, aa, ab);
        // where z=grid_x, aa=grid_y, ab=grid_z
        let mut rng = self.positional_random.at(grid_x, grid_y, grid_z);

        // Java: ae = BlockPos.asLong(
        //     fromGridX(z, randomSource.nextInt(10)),
        //     fromGridY(aa, randomSource.nextInt(9)),
        //     fromGridZ(ab, randomSource.nextInt(10))
        // );
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
    /// fluid level it should have.
    ///
    /// The algorithm samples preliminary surface level at 13 nearby positions
    /// (in chunk coordinates) to determine if aquifer water should exist.
    /// This prevents floating water above the terrain surface.
    ///
    /// Java reference: Aquifer.NoiseBasedAquifer.computeFluid()
    fn compute_fluid(&self, x: i32, y: i32, z: i32) -> FluidStatus {
        let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);

        // Track minimum raw surface and whether we're below any surface with fluid
        let mut min_surface_raw = i32::MAX; // Java: l = Integer.MAX_VALUE
        let y_upper = y + 12; // Java: m = j + 12
        let y_lower = y - 12; // Java: n = j - 12
        let mut is_below_surface_with_fluid = false; // Java: bl = false

        // Sample surface level at 13 nearby positions (matches Java's SURFACE_SAMPLING_OFFSETS_IN_CHUNKS)
        // These positions can be up to 3 chunks away, so we need to create FlatCacheGrids for them.
        for (i, [chunk_offset_x, chunk_offset_z]) in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS.iter().enumerate() {
            let sample_x = x + chunk_offset_x * 16; // Java: o = i + SectionPos.sectionToBlockCoord(is[0])
            let sample_z = z + chunk_offset_z * 16; // Java: p = k + SectionPos.sectionToBlockCoord(is[1])

            // Get preliminary surface level at this position
            // Java: int q = this.noiseChunk.preliminarySurfaceLevel(o, p)
            // Java's preliminarySurfaceLevel quantizes to quart boundaries first:
            //   int k = QuartPos.toBlock(QuartPos.fromBlock(i)) = (i >> 2) << 2
            let quart_x = (sample_x >> 2) << 2;
            let quart_z = (sample_z >> 2) << 2;
            let ctx = FunctionContext::new(quart_x, 0, quart_z);

            // Create FlatCacheGrid for the chunk containing this sample position
            let sample_chunk_x = quart_x >> 4;  // Divide by 16
            let sample_chunk_z = quart_z >> 4;  // Divide by 16
            let sample_grid = FlatCacheGrid::new(sample_chunk_x, sample_chunk_z, self.noises);
            let col = ColumnContext::new(quart_x, quart_z, self.noises, &sample_grid);
            let raw_surface = compute_preliminary_surface_level(&ctx, self.noises, &sample_grid, &col).floor() as i32; // Java: q
            let adjusted_surface = raw_surface + 8; // Java: r = this.adjustSurfaceLevel(q)

            let is_at_our_position = i == 0; // Java: bl2 = is[0] == 0 && is[1] == 0

            // Java: if (bl2 && n > r) return fluidStatus
            // If at our position and we're more than 12 blocks below the adjusted surface,
            // return global fluid (we're deep underground, use normal behavior)
            if is_at_our_position && y_lower > adjusted_surface {
                return global_fluid;
            }

            // Java: bl3 = m > r (are we above the adjusted surface?)
            let is_above_adjusted_surface = y_upper > adjusted_surface;

            // Java: if (bl3 || bl2) { ... }
            if is_above_adjusted_surface || is_at_our_position {
                // Get fluid at surface level
                // Java: FluidStatus fluidStatus2 = this.globalFluidPicker.computeFluid(o, r, p)
                let surface_fluid = self.global_fluid_picker.compute_fluid(sample_x, adjusted_surface, sample_z);

                // Java: if (!fluidStatus2.at(r).isAir()) { ... }
                if surface_fluid.at(adjusted_surface) != *blocks::AIR {
                    // Java: if (bl2) bl = true
                    if is_at_our_position {
                        is_below_surface_with_fluid = true;
                    }
                    // Java: if (bl3) return fluidStatus2
                    if is_above_adjusted_surface {
                        // We're above the surface and there's fluid at surface - return that fluid
                        return surface_fluid;
                    }
                }
            }

            // Java: l = Math.min(l, q)
            min_surface_raw = min_surface_raw.min(raw_surface);
        }

        // Java: int s = this.computeSurfaceLevel(i, j, k, fluidStatus, l, bl)
        let fluid_level = self.compute_surface_level(x, y, z, &global_fluid, min_surface_raw, is_below_surface_with_fluid);

        // Java: return new Aquifer.FluidStatus(s, this.computeFluidType(i, j, k, fluidStatus, s))
        FluidStatus::new(fluid_level, self.compute_fluid_type(x, y, z, &global_fluid, fluid_level))
    }

    /// Compute the fluid surface level for an aquifer.
    ///
    /// Java reference: Aquifer.NoiseBasedAquifer.computeSurfaceLevel()
    ///
    /// Note: The (x, y, z) coordinates are aquifer center positions which can be
    /// outside the current chunk, so we need to create FlatCacheGrid for them.
    fn compute_surface_level(
        &self,
        x: i32,
        y: i32,
        z: i32,
        global_fluid: &FluidStatus,
        min_surface_raw: i32,
        is_below_surface_with_fluid: bool,
    ) -> i32 {
        let ctx = FunctionContext::new(x, y, z);

        // Create FlatCacheGrid for the chunk containing this position
        let chunk_x = x >> 4;
        let chunk_z = z >> 4;
        let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
        let col = ColumnContext::new(x, z, self.noises, &grid);

        // Check for deep dark region - disables aquifer flooding in these areas
        // Java: if (OverworldBiomeBuilder.isDeepDarkRegion(this.erosion, this.depth, singlePointContext)) { d=-1; e=-1; }
        // When in deep dark region, both thresholds become -1.0, which with floodedness clamped to [-1,1]
        // means neither e>0 nor d>0 can be true, so we return WAY_BELOW_MIN_Y (no aquifer)
        if is_deep_dark_region(&ctx, self.noises, &grid, &col) {
            return WAY_BELOW_MIN_Y;
        }

        // Java: int m = l + 8 - j  (distance from adjusted min surface to current Y)
        let dist_from_surface = min_surface_raw + 8 - y;

        // Java: double f = bl ? Mth.clampedMap((double)m, 0.0, 64.0, 1.0, 0.0) : 0.0
        // When close to surface (m=0): f=1.0, when far (m>=64): f=0.0
        let surface_proximity = if is_below_surface_with_fluid {
            // clampedMap(m, 0, 64, 1, 0) = 1 - (clamp(m, 0, 64) / 64)
            let clamped = (dist_from_surface as f64).clamp(0.0, 64.0);
            1.0 - (clamped / 64.0)
        } else {
            0.0
        };

        // Java: double g = Mth.clamp(this.fluidLevelFloodednessNoise.compute(...), -1.0, 1.0)
        let floodedness_noise = compute_fluid_level_floodedness(&ctx, self.noises, &grid, &col).clamp(-1.0, 1.0);

        // Java: double h = Mth.map(f, 1.0, 0.0, -0.3, 0.8)
        // When f=1 (close to surface): h=-0.3, when f=0 (far): h=0.8
        // map(f, 1, 0, -0.3, 0.8) = -0.3 + (0.8 - (-0.3)) * (1 - f) / (0 - 1 + 1) wait...
        // Actually: map(value, fromMin, fromMax, toMin, toMax) = toMin + (toMax - toMin) * (value - fromMin) / (fromMax - fromMin)
        // map(f, 1.0, 0.0, -0.3, 0.8) = -0.3 + (0.8 - (-0.3)) * (f - 1.0) / (0.0 - 1.0)
        //                             = -0.3 + 1.1 * (f - 1.0) / (-1.0)
        //                             = -0.3 - 1.1 * (f - 1.0)
        //                             = -0.3 - 1.1*f + 1.1
        //                             = 0.8 - 1.1*f
        let threshold_h = 0.8 - 1.1 * surface_proximity;

        // Java: double o = Mth.map(f, 1.0, 0.0, -0.8, 0.4)
        // = 0.4 - 1.2 * f
        let threshold_o = 0.4 - 1.2 * surface_proximity;

        // Java: d = g - o, e = g - h
        let d = floodedness_noise - threshold_o; // Must be > 0 for partial flood
        let e = floodedness_noise - threshold_h; // Must be > 0 for full flood

        // Java logic:
        // if (e > 0.0) m = fluidStatus.fluidLevel  (FULLY FLOODED - use global level)
        // else if (d > 0.0) m = computeRandomizedFluidSurfaceLevel(...) (PARTIAL)
        // else m = WAY_BELOW_MIN_Y (NO AQUIFER)
        if e > 0.0 {
            // Fully flooded - use global fluid level
            global_fluid.fluid_level
        } else if d > 0.0 {
            // Partially flooded - compute randomized level
            self.compute_randomized_fluid_level(x, y, z, min_surface_raw)
        } else {
            // Not flooded - no aquifer here
            WAY_BELOW_MIN_Y
        }
    }

    /// Compute a randomized fluid surface level for partially flooded areas.
    ///
    /// Note: Uses grid-divided coordinates for coarse sampling, but chunk/column from original coords.
    fn compute_randomized_fluid_level(&self, x: i32, y: i32, z: i32, min_surface: i32) -> i32 {
        // Use coarser grid (16x40x16) for spread noise
        let grid_x = x.div_euclid(16);
        let grid_y = y.div_euclid(40);
        let grid_z = z.div_euclid(16);

        let base_level = grid_y * 40 + 20; // Center of the grid cell

        let ctx = FunctionContext::new(grid_x, grid_y, grid_z);

        // Create FlatCacheGrid for the chunk containing the ORIGINAL position (not grid position!)
        // BUG FIX: Was using grid_x >> 4, which double-divides by 256 instead of 16
        let chunk_x = x >> 4;
        let chunk_z = z >> 4;
        let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
        let col = ColumnContext::new(x, z, self.noises, &grid);
        let spread = compute_fluid_level_spread(&ctx, self.noises, &grid, &col) * 10.0;
        // Java: Mth.quantize(d, 3) = floor(d / 3) * 3
        let quantized = (spread / 3.0).floor() as i32 * 3;

        let level = base_level + quantized;
        // Never go above the minimum surface level (prevent floating water above terrain)
        level.min(min_surface)
    }

    /// Determine fluid type (water or lava) at a position.
    ///
    /// Note: Uses grid-divided coordinates for coarse sampling, but chunk/column from original coords.
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

            let ctx = FunctionContext::new(lx, ly, lz);

            // Create FlatCacheGrid for the chunk containing the ORIGINAL position (not grid position!)
            // BUG FIX: Was using lx >> 4, which divides grid coord by 16, giving wrong chunk
            let chunk_x = x >> 4;
            let chunk_z = z >> 4;
            let grid = FlatCacheGrid::new(chunk_x, chunk_z, self.noises);
            let col = ColumnContext::new(x, z, self.noises, &grid);
            let lava_value = compute_lava(&ctx, self.noises, &grid, &col);

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
    ///
    /// Java reference: Aquifer.NoiseBasedAquifer.calculatePressure() lines 303-361
    /// The pressure determines barrier formation between aquifers with different fluid levels.
    ///
    /// Java variables:
    /// - d = avgLevel = (level1 + level2) / 2
    /// - e = signedOffset = y + 0.5 - avgLevel
    /// - f = halfDiff = |level1 - level2| / 2
    /// - o = halfDiff - |signedOffset|
    /// - p = (e > 0) ? o : (3.0 + o)
    /// - q = (p > 0) ? p/divisor1 : p/divisor2
    fn calculate_pressure(
        &self,
        ctx: &FunctionContext,
        col: &ColumnContext,
        barrier_value: &mut f64,
        fluid1: &FluidStatus,
        fluid2: &FluidStatus,
    ) -> f64 {
        let y = ctx.block_y;
        let block1 = fluid1.at(y);
        let block2 = fluid2.at(y);

        // Lava/water interface creates strong barrier
        // Java: if ((status1 == LAVA && status2 == WATER) || vice versa) return 2.0
        if (block1 == *blocks::LAVA && block2 == *blocks::WATER)
            || (block1 == *blocks::WATER && block2 == *blocks::LAVA)
        {
            return 2.0;
        }

        // Java: int j = Math.abs(fluidStatus.fluidLevel - fluidStatus2.fluidLevel)
        // NOTE: When one fluid level is WAY_BELOW_MIN_Y, the level_diff will be huge,
        // resulting in a very large pressure that creates a barrier. This is intentional -
        // Java creates stone barriers between valid aquifers and non-existent ones.
        // Use i64 arithmetic to prevent overflow when dealing with WAY_BELOW_MIN_Y.
        let level_diff = ((fluid1.fluid_level as i64) - (fluid2.fluid_level as i64)).abs() as i32;
        if level_diff == 0 {
            return 0.0;
        }

        // Java formula (lines 314-323):
        // double d = 0.5 * (level1 + level2)   // avgLevel
        // double e = y + 0.5 - d               // signedOffset (signed distance from midpoint)
        // double f = level_diff / 2.0          // halfDiff
        // double o = f - Math.abs(e)           // halfDiff - |signedOffset|
        // Use f64 arithmetic to prevent i32 overflow when one level is WAY_BELOW_MIN_Y
        let avg_level = (fluid1.fluid_level as f64 + fluid2.fluid_level as f64) * 0.5;
        let signed_offset = y as f64 + 0.5 - avg_level;  // Java's 'e'
        let half_diff = level_diff as f64 * 0.5;          // Java's 'f'
        let o = half_diff - signed_offset.abs();          // Java's 'o'

        // Java formula (lines 325-338):
        // if (e > 0.0) {          // above midpoint
        //     double p = 0.0 + o;  // p = o
        //     q = p > 0 ? p/1.5 : p/2.5;
        // } else {                // below or at midpoint
        //     double p = 3.0 + o;
        //     q = p > 0 ? p/3.0 : p/10.0;
        // }
        let q = if signed_offset > 0.0 {
            // Above midpoint
            if o > 0.0 { o / 1.5 } else { o / 2.5 }
        } else {
            // Below midpoint (or at midpoint)
            let p = 3.0 + o;
            if p > 0.0 { p / 3.0 } else { p / 10.0 }
        };

        // Java (lines 343-353): Add barrier noise when q is in range [-2, 2]
        let barrier = if q >= -2.0 && q <= 2.0 {
            if barrier_value.is_nan() {
                *barrier_value = compute_barrier(ctx, self.noises, self.grid, col);
            }
            *barrier_value
        } else {
            0.0
        };

        // Java (line 354): return 2.0 * (barrier + q)
        2.0 * (barrier + q)
    }

    /// Insert into sorted arrays of 4 closest items.
    ///
    /// Java uses `>=` comparison, meaning when distances are equal, the new one
    /// takes precedence and pushes others down. This matches Java lines 203-227.
    fn insert_sorted(
        &self,
        indices: &mut [usize; 4],
        distances: &mut [i32; 4],
        new_index: usize,
        new_dist: i32,
    ) {
        // Java: if (o >= ai) - insert at position 0 if closer OR EQUAL
        // Java: else if (p >= ai) - insert at position 1
        // etc.
        if distances[0] >= new_dist {
            // Insert at position 0, shift all down
            indices[3] = indices[2];
            indices[2] = indices[1];
            indices[1] = indices[0];
            indices[0] = new_index;
            distances[3] = distances[2];
            distances[2] = distances[1];
            distances[1] = distances[0];
            distances[0] = new_dist;
        } else if distances[1] >= new_dist {
            // Insert at position 1, shift 2,3 down
            indices[3] = indices[2];
            indices[2] = indices[1];
            indices[1] = new_index;
            distances[3] = distances[2];
            distances[2] = distances[1];
            distances[1] = new_dist;
        } else if distances[2] >= new_dist {
            // Insert at position 2, shift 3 down
            indices[3] = indices[2];
            indices[2] = new_index;
            distances[3] = distances[2];
            distances[2] = new_dist;
        } else if distances[3] >= new_dist {
            // Insert at position 3
            indices[3] = new_index;
            distances[3] = new_dist;
        }
        // If new_dist > distances[3], don't insert
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
    pub fn compute_substance(&self, ctx: &FunctionContext, density: f64) -> Option<u32> {
        if density > 0.0 {
            None // Solid block
        } else {
            let y = ctx.block_y;
            Some(
                self.fluid_picker
                    .compute_fluid(ctx.block_x, y, ctx.block_z)
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

        let ctx = FunctionContext::new(0, 62, 0);

        // Below sea level with negative density = water
        let result = aquifer.compute_substance(&ctx, -1.0);
        assert_eq!(result, Some(*blocks::WATER));

        // Positive density = None (solid)
        let result = aquifer.compute_substance(&ctx, 1.0);
        assert_eq!(result, None);

        // Above sea level with negative density = air
        let ctx_above = FunctionContext::new(0, 64, 0);
        let result = aquifer.compute_substance(&ctx_above, -1.0);
        assert_eq!(result, Some(*blocks::AIR));

        assert!(!aquifer.should_schedule_fluid_update());
    }

    // TODO: Rewrite integration tests for NoiseBasedAquifer with new enum-based system
    // The following tests were removed during migration from Arc<dyn DensityFunction>
    // to the enum-based DensityFunction system:
    // - test_cave_position_should_be_air_not_water
    // - test_floodedness_distribution_at_y0
    // - test_aquifer_trace_at_y0
    // - test_aquifer_flooded_regions
    // - test_aquifer_cell_consistency
    // - test_aquifer_horizontal_consistency
}
