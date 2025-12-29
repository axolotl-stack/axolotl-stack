//! NoiseChunk - the cell-based interpolation engine.
//!
//! NoiseChunk manages the multi-level caching and trilinear interpolation
//! system that makes 3D density evaluation practical. It divides a chunk
//! into cells and uses sliding window slice caching to minimize noise evaluations.
//!
//! ## Cell Structure
//!
//! A chunk (16x384x16 blocks) is divided into cells:
//! - Cell width: 4 blocks (4 cells per chunk in XZ)
//! - Cell height: 8 blocks (48 cells for 384 block height)
//!
//! Density is evaluated at cell corners and trilinearly interpolated
//! for interior block positions.
//!
//! ## Traversal Order
//!
//! The chunk is traversed in a specific order to maximize cache reuse:
//! - X outer loop (allows slice swapping)
//! - Z middle loop
//! - Y inner loop (descending for efficient surface detection)

use super::cache::{Cache2D, CacheAllInCell, CacheOnce, FlatCache, NoiseInterpolator};
use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use super::markers::{
    Cache2DMarker, CacheAllInCellMarker, CacheOnceMarker, FlatCacheMarker, Interpolated,
};
use std::any::Any;
use std::sync::{Arc, RwLock};

/// Cell-based noise chunk for caching and interpolation.
///
/// This is the main orchestrator for density function evaluation within a chunk.
/// It manages:
/// - Cell configuration and traversal state
/// - Multiple cache types (interpolators, flat caches, etc.)
/// - Counter-based cache invalidation
pub struct NoiseChunk {
    // Cell configuration
    /// Width of each cell in blocks (typically 4).
    cell_width: i32,
    /// Height of each cell in blocks (typically 8).
    cell_height: i32,
    /// Number of cells in X/Z direction.
    cell_count_xz: i32,
    /// Number of cells in Y direction.
    cell_count_y: i32,

    // World position
    /// Minimum X block coordinate of this chunk.
    min_block_x: i32,
    /// Minimum Y block coordinate of this chunk.
    min_block_y: i32,
    /// Minimum Z block coordinate of this chunk.
    min_block_z: i32,

    // Current cell position state
    /// Current cell X index.
    cell_x: RwLock<i32>,
    /// Current cell Y index.
    cell_y: RwLock<i32>,
    /// Current cell Z index.
    cell_z: RwLock<i32>,

    // Position within cell
    /// Current position within cell (0 to cell_width-1).
    in_cell_x: RwLock<i32>,
    /// Current position within cell (0 to cell_height-1).
    in_cell_y: RwLock<i32>,
    /// Current position within cell (0 to cell_width-1).
    in_cell_z: RwLock<i32>,

    // Interpolation parameters
    /// Interpolation t value for Y axis (0.0 to 1.0).
    t_y: RwLock<f64>,
    /// Interpolation t value for X axis (0.0 to 1.0).
    t_x: RwLock<f64>,
    /// Interpolation t value for Z axis (0.0 to 1.0).
    t_z: RwLock<f64>,

    // Counters for cache invalidation
    /// Counter that increments each interpolation step.
    interpolation_counter: RwLock<u64>,
    /// Counter for array fill operations.
    array_counter: RwLock<u64>,

    // State flags
    /// Whether we're currently in interpolation mode.
    interpolating: RwLock<bool>,
    /// Whether we're filling a cell.
    filling_cell: RwLock<bool>,

    // Cache collections (populated during wrap())
    /// Interpolators for trilinear interpolation.
    interpolators: RwLock<Vec<Arc<NoiseInterpolator>>>,
    /// Flat XZ caches.
    flat_caches: RwLock<Vec<Arc<FlatCache>>>,
    /// Single-point 2D caches.
    cache_2d: RwLock<Vec<Arc<Cache2D>>>,
    /// Once-per-step caches.
    cache_once: RwLock<Vec<Arc<CacheOnce>>>,
    /// Cell precomputation caches.
    cell_caches: RwLock<Vec<Arc<CacheAllInCell>>>,
}

impl NoiseChunk {
    /// Create a new NoiseChunk for the given chunk position.
    ///
    /// # Arguments
    /// * `chunk_x` - Chunk X coordinate
    /// * `chunk_z` - Chunk Z coordinate
    /// * `cell_width` - Width of each cell in blocks (typically 4)
    /// * `cell_height` - Height of each cell in blocks (typically 8)
    /// * `min_y` - Minimum Y coordinate (e.g., -64)
    /// * `height` - Total height in blocks (e.g., 384)
    pub fn new(
        chunk_x: i32,
        chunk_z: i32,
        cell_width: i32,
        cell_height: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        let cell_count_xz = 16 / cell_width;
        let cell_count_y = height / cell_height;

        Self {
            cell_width,
            cell_height,
            cell_count_xz,
            cell_count_y,
            min_block_x: chunk_x * 16,
            min_block_y: min_y,
            min_block_z: chunk_z * 16,
            cell_x: RwLock::new(0),
            cell_y: RwLock::new(0),
            cell_z: RwLock::new(0),
            in_cell_x: RwLock::new(0),
            in_cell_y: RwLock::new(0),
            in_cell_z: RwLock::new(0),
            t_y: RwLock::new(0.0),
            t_x: RwLock::new(0.0),
            t_z: RwLock::new(0.0),
            interpolation_counter: RwLock::new(0),
            array_counter: RwLock::new(0),
            interpolating: RwLock::new(false),
            filling_cell: RwLock::new(false),
            interpolators: RwLock::new(Vec::new()),
            flat_caches: RwLock::new(Vec::new()),
            cache_2d: RwLock::new(Vec::new()),
            cache_once: RwLock::new(Vec::new()),
            cell_caches: RwLock::new(Vec::new()),
        }
    }

    /// Get cell dimensions.
    pub fn cell_width(&self) -> i32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> i32 {
        self.cell_height
    }

    pub fn cell_count_xz(&self) -> i32 {
        self.cell_count_xz
    }

    pub fn cell_count_y(&self) -> i32 {
        self.cell_count_y
    }

    /// Get the current interpolation counter.
    pub fn interpolation_counter(&self) -> u64 {
        *self.interpolation_counter.read().unwrap()
    }

    /// Check if currently interpolating.
    pub fn is_interpolating(&self) -> bool {
        *self.interpolating.read().unwrap()
    }

    // ========== Interpolation Control Methods ==========

    /// Initialize for the first cell in X.
    ///
    /// Fills slice0 for all interpolators at cell_x = 0.
    pub fn initialize_for_first_cell_x(&self) {
        *self.interpolating.write().unwrap() = true;
        *self.cell_x.write().unwrap() = 0;

        // Fill slice0 for all interpolators
        for interp in self.interpolators.read().unwrap().iter() {
            interp.fill_slice(
                true, // is_slice0
                self.min_block_x,
                self.min_block_y,
                self.cell_height,
                self.min_block_z,
                self.cell_width,
                self.cell_count_xz as usize,
                self.cell_count_y as usize,
            );
        }
    }

    /// Advance to the next cell in X.
    ///
    /// Fills slice1 for the new X position. Call swap_slices() after
    /// processing this cell to prepare for the next.
    pub fn advance_cell_x(&self, cell_x: i32) {
        *self.cell_x.write().unwrap() = cell_x;

        // Fill slice1 with the next X slice
        let block_x = self.min_block_x + (cell_x + 1) * self.cell_width;
        for interp in self.interpolators.read().unwrap().iter() {
            interp.fill_slice(
                false, // is_slice1
                block_x,
                self.min_block_y,
                self.cell_height,
                self.min_block_z,
                self.cell_width,
                self.cell_count_xz as usize,
                self.cell_count_y as usize,
            );
        }
    }

    /// Select a cell by Y and Z indices.
    ///
    /// Loads the 8 corner values for interpolation.
    pub fn select_cell_yz(&self, cell_y: i32, cell_z: i32) {
        *self.cell_y.write().unwrap() = cell_y;
        *self.cell_z.write().unwrap() = cell_z;

        // Select corners for all interpolators
        for interp in self.interpolators.read().unwrap().iter() {
            interp.select_cell_yz(cell_y as usize, cell_z as usize);
        }

        // Fill cell caches if any
        let base_x = self.min_block_x + *self.cell_x.read().unwrap() * self.cell_width;
        let base_y = self.min_block_y + cell_y * self.cell_height;
        let base_z = self.min_block_z + cell_z * self.cell_width;

        *self.filling_cell.write().unwrap() = true;
        for cache in self.cell_caches.read().unwrap().iter() {
            cache.fill_cell(base_x, base_y, base_z);
        }
        *self.filling_cell.write().unwrap() = false;
    }

    /// Update interpolation for Y position within cell.
    ///
    /// # Arguments
    /// * `y_in_cell` - Position within cell (0 to cell_height-1)
    /// * `t` - Interpolation parameter (0.0 to 1.0)
    pub fn update_for_y(&self, y_in_cell: i32, t: f64) {
        *self.in_cell_y.write().unwrap() = y_in_cell;
        *self.t_y.write().unwrap() = t;

        for interp in self.interpolators.read().unwrap().iter() {
            interp.update_for_y(t);
        }
    }

    /// Update interpolation for X position within cell.
    ///
    /// # Arguments
    /// * `x_in_cell` - Position within cell (0 to cell_width-1)
    /// * `t` - Interpolation parameter (0.0 to 1.0)
    pub fn update_for_x(&self, x_in_cell: i32, t: f64) {
        *self.in_cell_x.write().unwrap() = x_in_cell;
        *self.t_x.write().unwrap() = t;

        for interp in self.interpolators.read().unwrap().iter() {
            interp.update_for_x(t);
        }
    }

    /// Update interpolation for Z position within cell.
    ///
    /// This also increments the interpolation counter.
    ///
    /// # Arguments
    /// * `z_in_cell` - Position within cell (0 to cell_width-1)
    /// * `t` - Interpolation parameter (0.0 to 1.0)
    pub fn update_for_z(&self, z_in_cell: i32, t: f64) {
        *self.in_cell_z.write().unwrap() = z_in_cell;
        *self.t_z.write().unwrap() = t;

        for interp in self.interpolators.read().unwrap().iter() {
            interp.update_for_z(t);
        }

        // Increment counter for cache invalidation
        *self.interpolation_counter.write().unwrap() += 1;
    }

    /// Swap slices after completing a cell X iteration.
    pub fn swap_slices(&self) {
        for interp in self.interpolators.read().unwrap().iter() {
            interp.swap_slices();
        }
    }

    /// Stop interpolation mode.
    pub fn stop_interpolation(&self) {
        *self.interpolating.write().unwrap() = false;
    }

    // ========== Cache Registration ==========

    /// Register an interpolator.
    pub fn register_interpolator(&self, interp: Arc<NoiseInterpolator>) {
        self.interpolators.write().unwrap().push(interp);
    }

    /// Register a flat cache.
    pub fn register_flat_cache(&self, cache: Arc<FlatCache>) {
        self.flat_caches.write().unwrap().push(cache);
    }

    /// Register a 2D cache.
    pub fn register_cache_2d(&self, cache: Arc<Cache2D>) {
        self.cache_2d.write().unwrap().push(cache);
    }

    /// Register a cache-once.
    pub fn register_cache_once(&self, cache: Arc<CacheOnce>) {
        self.cache_once.write().unwrap().push(cache);
    }

    /// Register a cell cache.
    pub fn register_cell_cache(&self, cache: Arc<CacheAllInCell>) {
        self.cell_caches.write().unwrap().push(cache);
    }
}

impl FunctionContext for NoiseChunk {
    fn block_x(&self) -> i32 {
        self.min_block_x
            + *self.cell_x.read().unwrap() * self.cell_width
            + *self.in_cell_x.read().unwrap()
    }

    fn block_y(&self) -> i32 {
        self.min_block_y
            + *self.cell_y.read().unwrap() * self.cell_height
            + *self.in_cell_y.read().unwrap()
    }

    fn block_z(&self) -> i32 {
        self.min_block_z
            + *self.cell_z.read().unwrap() * self.cell_width
            + *self.in_cell_z.read().unwrap()
    }
}

/// Visitor that wraps density functions with cache implementations.
///
/// When applied to a density function graph, this visitor replaces
/// marker types (Interpolated, FlatCacheMarker, etc.) with actual
/// cache implementations that are registered with the NoiseChunk.
pub struct WrapVisitor<'a> {
    /// Reference to the NoiseChunk for cache registration.
    noise_chunk: &'a NoiseChunk,
}

impl<'a> WrapVisitor<'a> {
    /// Create a new wrap visitor.
    pub fn new(noise_chunk: &'a NoiseChunk) -> Self {
        Self { noise_chunk }
    }
}

impl<'a> Visitor for WrapVisitor<'a> {
    fn apply(&self, func: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        // Check if this is a marker type that needs wrapping
        // We use downcasting to identify marker types

        // For now, return the function as-is
        // The actual marker replacement will be done when we have type IDs
        func
    }
}

/// Visitor that creates interpolator wrappers for Interpolated markers.
pub struct InterpolatorWrapVisitor<'a> {
    noise_chunk: &'a NoiseChunk,
}

impl<'a> InterpolatorWrapVisitor<'a> {
    pub fn new(noise_chunk: &'a NoiseChunk) -> Self {
        Self { noise_chunk }
    }
}

impl<'a> Visitor for InterpolatorWrapVisitor<'a> {
    fn apply(&self, func: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        // Create an interpolator for this function
        let interp = Arc::new(NoiseInterpolator::new(
            func,
            self.noise_chunk.cell_count_xz as usize,
            self.noise_chunk.cell_count_y as usize,
        ));
        self.noise_chunk.register_interpolator(interp.clone());
        interp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::math::Constant;

    #[test]
    fn test_noise_chunk_creation() {
        let chunk = NoiseChunk::new(0, 0, 4, 8, -64, 384);

        assert_eq!(chunk.cell_width(), 4);
        assert_eq!(chunk.cell_height(), 8);
        assert_eq!(chunk.cell_count_xz(), 4); // 16 / 4
        assert_eq!(chunk.cell_count_y(), 48); // 384 / 8
    }

    #[test]
    fn test_noise_chunk_block_coords() {
        let chunk = NoiseChunk::new(1, 2, 4, 8, -64, 384);

        // Initial position should be at min coords
        assert_eq!(chunk.block_x(), 16); // chunk_x * 16 = 1 * 16
        assert_eq!(chunk.block_y(), -64);
        assert_eq!(chunk.block_z(), 32); // chunk_z * 16 = 2 * 16

        // Update position within cell
        *chunk.cell_x.write().unwrap() = 1;
        *chunk.in_cell_x.write().unwrap() = 2;
        assert_eq!(chunk.block_x(), 16 + 4 + 2); // min + cell_x*4 + in_cell

        *chunk.cell_y.write().unwrap() = 5;
        *chunk.in_cell_y.write().unwrap() = 3;
        assert_eq!(chunk.block_y(), -64 + 40 + 3); // min + cell_y*8 + in_cell
    }

    #[test]
    fn test_interpolation_counter() {
        let chunk = NoiseChunk::new(0, 0, 4, 8, -64, 384);

        assert_eq!(chunk.interpolation_counter(), 0);

        chunk.update_for_z(0, 0.0);
        assert_eq!(chunk.interpolation_counter(), 1);

        chunk.update_for_z(1, 0.25);
        assert_eq!(chunk.interpolation_counter(), 2);
    }

    #[test]
    fn test_noise_chunk_with_interpolator() {
        let chunk = NoiseChunk::new(0, 0, 4, 8, -64, 384);

        // Create a simple constant function
        let func = Arc::new(Constant::new(1.0));

        // Create and register an interpolator
        let interp = Arc::new(NoiseInterpolator::new(func, 4, 48));
        chunk.register_interpolator(interp);

        // Should have one interpolator
        assert_eq!(chunk.interpolators.read().unwrap().len(), 1);
    }

    #[test]
    fn test_cell_traversal_simulation() {
        let chunk = NoiseChunk::new(0, 0, 4, 8, -64, 384);

        // Create a constant density function
        let func = Arc::new(Constant::new(5.0));
        let interp = Arc::new(NoiseInterpolator::new(func, 4, 48));
        chunk.register_interpolator(interp.clone());

        // Simulate traversal
        chunk.initialize_for_first_cell_x();
        assert!(chunk.is_interpolating());

        // Advance through first X cell
        chunk.advance_cell_x(0);

        // Select a cell
        chunk.select_cell_yz(24, 2); // Middle of world

        // Update for a position
        chunk.update_for_y(4, 0.5);
        chunk.update_for_x(2, 0.5);
        chunk.update_for_z(2, 0.5);

        // The interpolator should have a value
        let value = interp.get_value();
        // With constant input, output should also be constant
        assert!((value - 5.0).abs() < 0.001);
    }
}
