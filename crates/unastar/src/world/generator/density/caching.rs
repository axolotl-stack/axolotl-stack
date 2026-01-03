//! Caching system for density function evaluation.
//!
//! This module implements Java Edition's caching strategy for density functions.
//! The key insight is that density functions marked with `Interpolated` should be
//! evaluated at cell corners only and trilinearly interpolated for interior blocks.
//!
//! ## Caching Strategy
//!
//! Java uses several cache types:
//! - `NoiseInterpolator`: Pre-computes at cell corners, trilinearly interpolates
//! - `FlatCache`: Pre-computes entire XZ grid for Y-independent functions
//! - `Cache2D`: Memoizes last (X, Z) position and value per cache node
//!
//! ## Our Approach
//!
//! We use AOT-compiled density functions from `unastar_noise` that directly compute
//! all values without tree traversal. The `CellInterpolator` pre-computes at cell
//! corners and trilinearly interpolates for interior blocks.

use super::types::NoiseRegistry;
use super::lerp3;
use std::simd::prelude::*;
use unastar_noise::{FunctionContext, FunctionContext4, FlatCacheGrid, ColumnContext, ColumnContextGrid, compute_final_density, compute_final_density_4};

/// Pre-computed XZ grid for FlatCache (Y-independent functions).
///
/// FlatCache is used for functions like continentalness, erosion, temperature, etc.
/// that only depend on X and Z coordinates. The grid stores values at quart positions
/// (block >> 2) and is pre-computed once when first accessed.
#[derive(Clone, Copy)]
pub struct FlatCacheData {
    /// Pre-computed XZ grid (5x5 quart positions for 16x16 block chunk).
    /// Indexed as values[qz_offset][qx_offset] where offset is relative to first_quart_x/z.
    pub values: [[f64; 5]; 5],
    /// First quart X coordinate (chunk_x * 16 >> 2 = chunk_x * 4).
    pub first_quart_x: i32,
    /// First quart Z coordinate (chunk_z * 16 >> 2 = chunk_z * 4).
    pub first_quart_z: i32,
    /// Whether the grid has been initialized.
    pub initialized: bool,
}

impl FlatCacheData {
    /// Create new uninitialized FlatCacheData for the given chunk.
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            values: [[0.0; 5]; 5],
            first_quart_x: chunk_x * 4, // chunk * 16 / 4
            first_quart_z: chunk_z * 4,
            initialized: false,
        }
    }
}

/// Per-chunk cache state for density function evaluation.
///
/// This is a simplified version that works with AOT-compiled functions.
pub struct ChunkCache {
    /// Chunk X coordinate.
    pub chunk_x: i32,
    /// Chunk Z coordinate.
    pub chunk_z: i32,
}

impl ChunkCache {
    /// Create a new chunk cache for the given chunk position.
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self { chunk_x, chunk_z }
    }
}

/// Maximum cell count in Y direction (384 / 8 = 48, plus 1 for corners).
const MAX_SLICE_HEIGHT: usize = 49;
/// Maximum cell count in XZ direction (16 / 4 = 4, plus 1 for corners).
const MAX_SLICE_WIDTH: usize = 5;

/// A cell interpolator that pre-computes density at cell corners.
///
/// This is the core of the Java-style caching. For an `Interpolated` density function,
/// we compute values at the 8 corners of a cell and trilinearly interpolate for all
/// 128 interior blocks (4x8x4).
///
/// Uses fixed-size arrays to avoid heap allocations during chunk generation.
pub struct CellInterpolator {
    /// Double-buffered slices for X advancement.
    /// slice0[z][y] contains values for current X, slice1[z][y] for next X.
    /// Fixed-size to avoid Vec allocation overhead.
    slice0: [[f64; MAX_SLICE_HEIGHT]; MAX_SLICE_WIDTH],
    slice1: [[f64; MAX_SLICE_HEIGHT]; MAX_SLICE_WIDTH],

    /// Cached corner values for current cell
    noise000: f64,
    noise001: f64,
    noise100: f64,
    noise101: f64,
    noise010: f64,
    noise011: f64,
    noise110: f64,
    noise111: f64,

    /// Interpolated values (updated incrementally)
    value_xz00: f64,
    value_xz10: f64,
    value_xz01: f64,
    value_xz11: f64,
    value_z0: f64,
    value_z1: f64,

    /// Current interpolated value
    pub value: f64,
}

impl CellInterpolator {
    /// Create a new cell interpolator for the given cell counts.
    /// Uses fixed-size arrays - no heap allocation.
    pub fn new(_cell_count_y: usize, _cell_count_xz: usize) -> Self {
        // Note: cell_count_y and cell_count_xz are no longer used since we use fixed arrays.
        // They were 48 and 4 respectively, so our MAX_SLICE_HEIGHT=49 and MAX_SLICE_WIDTH=5 fit.
        Self {
            slice0: [[0.0; MAX_SLICE_HEIGHT]; MAX_SLICE_WIDTH],
            slice1: [[0.0; MAX_SLICE_HEIGHT]; MAX_SLICE_WIDTH],
            noise000: 0.0,
            noise001: 0.0,
            noise100: 0.0,
            noise101: 0.0,
            noise010: 0.0,
            noise011: 0.0,
            noise110: 0.0,
            noise111: 0.0,
            value_xz00: 0.0,
            value_xz10: 0.0,
            value_xz01: 0.0,
            value_xz11: 0.0,
            value_z0: 0.0,
            value_z1: 0.0,
            value: 0.0,
        }
    }

    /// Fill a slice using AOT compiled functions (no arena traversal).
    ///
    /// This is significantly faster than interpreting a density function tree
    /// because it uses ahead-of-time compiled Rust code.
    ///
    /// OPTIMIZATION: Uses pre-computed ColumnContextGrid for O(1) column context lookups
    /// instead of creating new ColumnContext objects for each column.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_slice_aot(
        &mut self,
        use_slice0: bool,
        cell_x: i32,
        first_cell_z: i32,
        cell_noise_min_y: i32,
        cell_count_y: usize,
        cell_count_xz: usize,
        cell_width: i32,
        cell_height: i32,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
        noises: &NoiseRegistry,
    ) {
        let cell_start_x = cell_x * cell_width;

        for z_idx in 0..=cell_count_xz {
            let cell_z = first_cell_z + z_idx as i32;
            let cell_start_z = cell_z * cell_width;

            let slice = if use_slice0 { &mut self.slice0 } else { &mut self.slice1 };

            // Use pre-computed ColumnContext from grid - O(1) lookup instead of expensive creation
            let col_ctx = col_grid.get_block(cell_start_x, cell_start_z);

            // Process 4 Y values at a time using AOT compiled SIMD function
            let mut y_idx = 0;
            while y_idx + 4 <= cell_count_y + 1 {
                let y0 = (cell_noise_min_y + y_idx as i32) * cell_height;
                let y1 = (cell_noise_min_y + y_idx as i32 + 1) * cell_height;
                let y2 = (cell_noise_min_y + y_idx as i32 + 2) * cell_height;
                let y3 = (cell_noise_min_y + y_idx as i32 + 3) * cell_height;

                // Use AOT compiled function directly - no arena traversal
                let ctx4 = FunctionContext4::new(cell_start_x, [y0, y1, y2, y3], cell_start_z);
                let results = compute_final_density_4(&ctx4, noises, grid, col_ctx).to_array();

                slice[z_idx][y_idx] = results[0];
                slice[z_idx][y_idx + 1] = results[1];
                slice[z_idx][y_idx + 2] = results[2];
                slice[z_idx][y_idx + 3] = results[3];

                y_idx += 4;
            }

            // Handle remaining Y values (if any)
            while y_idx <= cell_count_y {
                let cell_y = cell_noise_min_y + y_idx as i32;
                let cell_start_y = cell_y * cell_height;

                // Use AOT compiled scalar function
                let ctx = FunctionContext::new(cell_start_x, cell_start_y, cell_start_z);
                slice[z_idx][y_idx] = compute_final_density(&ctx, noises, grid, col_ctx);
                y_idx += 1;
            }
        }
    }

    /// Select a cell's corner values from the slices.
    #[inline]
    pub fn select_cell_yz(&mut self, cell_y: usize, cell_z: usize) {
        self.noise000 = self.slice0[cell_z][cell_y];
        self.noise001 = self.slice0[cell_z + 1][cell_y];
        self.noise100 = self.slice1[cell_z][cell_y];
        self.noise101 = self.slice1[cell_z + 1][cell_y];
        self.noise010 = self.slice0[cell_z][cell_y + 1];
        self.noise011 = self.slice0[cell_z + 1][cell_y + 1];
        self.noise110 = self.slice1[cell_z][cell_y + 1];
        self.noise111 = self.slice1[cell_z + 1][cell_y + 1];
    }

    /// Update interpolation for Y position (0.0 to 1.0).
    #[inline]
    pub fn update_for_y(&mut self, t: f64) {
        self.value_xz00 = lerp(t, self.noise000, self.noise010);
        self.value_xz10 = lerp(t, self.noise100, self.noise110);
        self.value_xz01 = lerp(t, self.noise001, self.noise011);
        self.value_xz11 = lerp(t, self.noise101, self.noise111);
    }

    /// Update interpolation for X position (0.0 to 1.0).
    #[inline]
    pub fn update_for_x(&mut self, t: f64) {
        self.value_z0 = lerp(t, self.value_xz00, self.value_xz10);
        self.value_z1 = lerp(t, self.value_xz01, self.value_xz11);
    }

    /// Update interpolation for Z position (0.0 to 1.0).
    #[inline]
    pub fn update_for_z(&mut self, t: f64) {
        self.value = lerp(t, self.value_z0, self.value_z1);
    }

    /// Get densities for all 4 Z positions in the cell at once (SIMD).
    ///
    /// This computes the final Z interpolation for z_in_cell = 0, 1, 2, 3
    /// using SIMD, avoiding 4 separate scalar lerps.
    ///
    /// `cell_width` is typically 4, so t values are 0.0, 0.25, 0.5, 0.75.
    #[inline]
    pub fn get_densities_4z(&self, cell_width: i32) -> f64x4 {
        // t values for z_in_cell = 0, 1, 2, 3
        let inv_width = 1.0 / cell_width as f64;
        let t = f64x4::from_array([0.0, inv_width, 2.0 * inv_width, 3.0 * inv_width]);

        // lerp(t, value_z0, value_z1) = value_z0 + t * (value_z1 - value_z0)
        let z0 = f64x4::splat(self.value_z0);
        let diff = f64x4::splat(self.value_z1 - self.value_z0);
        z0 + t * diff
    }

    /// Swap slices (called when advancing X).
    pub fn swap_slices(&mut self) {
        std::mem::swap(&mut self.slice0, &mut self.slice1);
    }

    /// Get interpolated value using direct lerp3 (for fillingCell mode).
    #[inline]
    pub fn interpolate_direct(&self, tx: f64, ty: f64, tz: f64) -> f64 {
        lerp3(
            tx, ty, tz,
            self.noise000, self.noise100,
            self.noise010, self.noise110,
            self.noise001, self.noise101,
            self.noise011, self.noise111,
        )
    }
}

/// Linear interpolation.
#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// Caching noise chunk that manages all caches for a chunk generation.
pub struct CachingNoiseChunk {
    // Cell configuration
    cell_width: i32,
    cell_height: i32,
    cell_count_xz: usize,
    cell_count_y: usize,
    cell_noise_min_y: i32,
    first_cell_x: i32,
    first_cell_z: i32,

    // Interpolator for final_density (the main one that matters)
    pub final_density_interpolator: CellInterpolator,

    // Current state
    cell_start_x: i32,
    cell_start_y: i32,
    cell_start_z: i32,
    in_cell_x: i32,
    in_cell_y: i32,
    in_cell_z: i32,

    /// Counter for cache invalidation
    interpolation_counter: u64,

    /// Whether we're currently interpolating
    interpolating: bool,
}

impl CachingNoiseChunk {
    /// Create a new caching noise chunk.
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        cell_width: i32,
        cell_height: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        let cell_count_xz = (16 / cell_width) as usize;
        let cell_count_y = (height / cell_height) as usize;
        let cell_noise_min_y = min_y / cell_height;
        let first_cell_x = (chunk_x * 16) / cell_width;
        let first_cell_z = (chunk_z * 16) / cell_width;

        Self {
            cell_width,
            cell_height,
            cell_count_xz,
            cell_count_y,
            cell_noise_min_y,
            first_cell_x,
            first_cell_z,
            final_density_interpolator: CellInterpolator::new(cell_count_y, cell_count_xz),
            cell_start_x: first_cell_x * cell_width,
            cell_start_y: cell_noise_min_y * cell_height,
            cell_start_z: first_cell_z * cell_width,
            in_cell_x: 0,
            in_cell_y: 0,
            in_cell_z: 0,
            interpolation_counter: 0,
            interpolating: false,
        }
    }

    /// Initialize for the first cell X using AOT compiled functions.
    ///
    /// This is the fast path that uses ahead-of-time compiled density functions.
    /// Uses pre-computed ColumnContextGrid for O(1) column context lookups.
    pub fn initialize_for_first_cell_x_aot(
        &mut self,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
        noises: &NoiseRegistry,
    ) {
        self.interpolating = true;
        self.interpolation_counter = 0;

        self.final_density_interpolator.fill_slice_aot(
            true, // use slice0
            self.first_cell_x,
            self.first_cell_z,
            self.cell_noise_min_y,
            self.cell_count_y,
            self.cell_count_xz,
            self.cell_width,
            self.cell_height,
            grid,
            col_grid,
            noises,
        );
    }

    /// Advance to next cell X using AOT compiled functions.
    /// Uses pre-computed ColumnContextGrid for O(1) column context lookups.
    pub fn advance_cell_x_aot(
        &mut self,
        cell_x_offset: i32,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
        noises: &NoiseRegistry,
    ) {
        // Fill slice1 with next X position using AOT compiled function
        self.final_density_interpolator.fill_slice_aot(
            false, // use slice1
            self.first_cell_x + cell_x_offset + 1,
            self.first_cell_z,
            self.cell_noise_min_y,
            self.cell_count_y,
            self.cell_count_xz,
            self.cell_width,
            self.cell_height,
            grid,
            col_grid,
            noises,
        );

        self.cell_start_x = (self.first_cell_x + cell_x_offset) * self.cell_width;
    }

    /// Select a cell by Y and Z indices.
    pub fn select_cell_yz(&mut self, cell_y: usize, cell_z: usize) {
        self.final_density_interpolator.select_cell_yz(cell_y, cell_z);

        self.cell_start_y = (cell_y as i32 + self.cell_noise_min_y) * self.cell_height;
        self.cell_start_z = (self.first_cell_z + cell_z as i32) * self.cell_width;
    }

    /// Update for Y position within cell.
    #[inline]
    pub fn update_for_y(&mut self, block_y: i32) {
        self.in_cell_y = block_y - self.cell_start_y;
        let t = self.in_cell_y as f64 / self.cell_height as f64;
        self.final_density_interpolator.update_for_y(t);
    }

    /// Update for X position within cell.
    #[inline]
    pub fn update_for_x(&mut self, block_x: i32) {
        self.in_cell_x = block_x - self.cell_start_x;
        let t = self.in_cell_x as f64 / self.cell_width as f64;
        self.final_density_interpolator.update_for_x(t);
    }

    /// Update for Z position within cell.
    #[inline]
    pub fn update_for_z(&mut self, block_z: i32) {
        self.in_cell_z = block_z - self.cell_start_z;
        self.interpolation_counter += 1;
        let t = self.in_cell_z as f64 / self.cell_width as f64;
        self.final_density_interpolator.update_for_z(t);
    }

    /// Get interpolated density at current position.
    #[inline]
    pub fn get_density(&self) -> f64 {
        self.final_density_interpolator.value
    }

    /// Get densities for all 4 Z positions at current (X, Y) using SIMD.
    ///
    /// Returns densities for z_in_cell = 0, 1, 2, 3.
    #[inline]
    pub fn get_densities_4z(&self) -> f64x4 {
        self.final_density_interpolator.get_densities_4z(self.cell_width)
    }

    /// Swap slices after processing a cell X column.
    pub fn swap_slices(&mut self) {
        self.final_density_interpolator.swap_slices();
    }

    /// Get current block X.
    #[inline]
    pub fn block_x(&self) -> i32 {
        self.cell_start_x + self.in_cell_x
    }

    /// Get current block Y.
    #[inline]
    pub fn block_y(&self) -> i32 {
        self.cell_start_y + self.in_cell_y
    }

    /// Get current block Z.
    #[inline]
    pub fn block_z(&self) -> i32 {
        self.cell_start_z + self.in_cell_z
    }

    /// Create a function context for the current position.
    #[inline]
    pub fn context(&self) -> FunctionContext {
        FunctionContext::new(self.block_x(), self.block_y(), self.block_z())
    }

    /// Get cell dimensions.
    pub fn cell_width(&self) -> i32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }

    pub fn cell_count_xz(&self) -> usize {
        self.cell_count_xz
    }

    pub fn cell_count_y(&self) -> usize {
        self.cell_count_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp() {
        assert!((lerp(0.0, 0.0, 1.0) - 0.0).abs() < 0.001);
        assert!((lerp(1.0, 0.0, 1.0) - 1.0).abs() < 0.001);
        assert!((lerp(0.5, 0.0, 1.0) - 0.5).abs() < 0.001);
        assert!((lerp(0.5, 2.0, 4.0) - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_cell_interpolator_creation() {
        let interp = CellInterpolator::new(48, 4);
        assert_eq!(interp.slice0.len(), 5); // cell_count_xz + 1
        assert_eq!(interp.slice0[0].len(), 49); // cell_count_y + 1
    }

    #[test]
    fn test_caching_noise_chunk_creation() {
        let chunk = CachingNoiseChunk::new(0, 0, 4, 8, -64, 384);
        assert_eq!(chunk.cell_count_xz, 4);
        assert_eq!(chunk.cell_count_y, 48);
    }
}
