//! Cache implementations for density functions.
//!
//! These are the actual cache implementations that replace the marker types
//! when a NoiseChunk is wired. They provide various caching strategies to
//! avoid redundant noise evaluations.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use super::math::lerp;
use std::sync::{Arc, RwLock};

/// Cache that stores a single 2D (XZ) position.
///
/// Returns the cached value if the same XZ is queried, otherwise recomputes.
pub struct Cache2D {
    /// The wrapped density function.
    wrapped: Arc<dyn DensityFunction>,
    /// Last cached X coordinate.
    last_x: RwLock<i32>,
    /// Last cached Z coordinate.
    last_z: RwLock<i32>,
    /// Last cached value.
    last_value: RwLock<f64>,
}

impl Cache2D {
    /// Create a new 2D position cache.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self {
            wrapped,
            last_x: RwLock::new(i32::MIN),
            last_z: RwLock::new(i32::MIN),
            last_value: RwLock::new(0.0),
        }
    }
}

impl DensityFunction for Cache2D {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let x = ctx.block_x();
        let z = ctx.block_z();

        // Check cache
        {
            let last_x = self.last_x.read().unwrap();
            let last_z = self.last_z.read().unwrap();
            if x == *last_x && z == *last_z {
                return *self.last_value.read().unwrap();
            }
        }

        // Compute new value
        let value = self.wrapped.compute(ctx);
        *self.last_x.write().unwrap() = x;
        *self.last_z.write().unwrap() = z;
        *self.last_value.write().unwrap() = value;
        value
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(Cache2D::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

/// Cache that stores the value once per interpolation counter update.
///
/// Caches the last computed value and only recomputes when the counter changes.
pub struct CacheOnce {
    /// The wrapped density function.
    wrapped: Arc<dyn DensityFunction>,
    /// Last interpolation counter value.
    last_counter: RwLock<u64>,
    /// Last cached value.
    last_value: RwLock<f64>,
}

impl CacheOnce {
    /// Create a new once-per-step cache.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self {
            wrapped,
            last_counter: RwLock::new(u64::MAX),
            last_value: RwLock::new(0.0),
        }
    }

    /// Get value for the given counter, computing if needed.
    pub fn get_or_compute(&self, counter: u64, ctx: &dyn FunctionContext) -> f64 {
        // Check cache
        {
            let last_counter = self.last_counter.read().unwrap();
            if counter == *last_counter {
                return *self.last_value.read().unwrap();
            }
        }

        // Compute new value
        let value = self.wrapped.compute(ctx);
        *self.last_counter.write().unwrap() = counter;
        *self.last_value.write().unwrap() = value;
        value
    }
}

impl DensityFunction for CacheOnce {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Without counter context, always compute
        self.wrapped.compute(ctx)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(CacheOnce::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

/// Trilinear interpolation cache.
///
/// Stores two XZ slices of density values for sliding window interpolation.
/// Values are computed at cell corners and interpolated for interior points.
pub struct NoiseInterpolator {
    /// The wrapped density function.
    wrapped: Arc<dyn DensityFunction>,

    /// First XZ slice: [cell_count_z + 1][cell_count_y + 1]
    slice0: RwLock<Vec<Vec<f64>>>,
    /// Second XZ slice
    slice1: RwLock<Vec<Vec<f64>>>,

    /// 8 corner values for current cell
    noise_000: RwLock<f64>,
    noise_001: RwLock<f64>,
    noise_010: RwLock<f64>,
    noise_011: RwLock<f64>,
    noise_100: RwLock<f64>,
    noise_101: RwLock<f64>,
    noise_110: RwLock<f64>,
    noise_111: RwLock<f64>,

    /// Progressive interpolation results (after Y lerp: 4 values)
    value_xz00: RwLock<f64>,
    value_xz01: RwLock<f64>,
    value_xz10: RwLock<f64>,
    value_xz11: RwLock<f64>,

    /// After X lerp: 2 values
    value_z0: RwLock<f64>,
    value_z1: RwLock<f64>,

    /// Final interpolated value
    value: RwLock<f64>,
}

impl NoiseInterpolator {
    /// Create a new noise interpolator.
    pub fn new(
        wrapped: Arc<dyn DensityFunction>,
        cell_count_z: usize,
        cell_count_y: usize,
    ) -> Self {
        let slice = vec![vec![0.0; cell_count_y + 1]; cell_count_z + 1];
        Self {
            wrapped,
            slice0: RwLock::new(slice.clone()),
            slice1: RwLock::new(slice),
            noise_000: RwLock::new(0.0),
            noise_001: RwLock::new(0.0),
            noise_010: RwLock::new(0.0),
            noise_011: RwLock::new(0.0),
            noise_100: RwLock::new(0.0),
            noise_101: RwLock::new(0.0),
            noise_110: RwLock::new(0.0),
            noise_111: RwLock::new(0.0),
            value_xz00: RwLock::new(0.0),
            value_xz01: RwLock::new(0.0),
            value_xz10: RwLock::new(0.0),
            value_xz11: RwLock::new(0.0),
            value_z0: RwLock::new(0.0),
            value_z1: RwLock::new(0.0),
            value: RwLock::new(0.0),
        }
    }

    /// Fill a slice with density values at cell corners.
    ///
    /// # Arguments
    /// * `is_slice0` - Whether to fill slice0 (true) or slice1 (false)
    /// * `block_x` - Block X coordinate for this slice
    /// * `min_y` - Minimum Y block coordinate
    /// * `cell_height` - Height of each cell in blocks
    /// * `min_z` - Minimum Z block coordinate
    /// * `cell_width` - Width of each cell in blocks
    /// * `cell_count_z` - Number of cells in Z direction
    /// * `cell_count_y` - Number of cells in Y direction
    pub fn fill_slice(
        &self,
        is_slice0: bool,
        block_x: i32,
        min_y: i32,
        cell_height: i32,
        min_z: i32,
        cell_width: i32,
        cell_count_z: usize,
        cell_count_y: usize,
    ) {
        let mut slice = if is_slice0 {
            self.slice0.write().unwrap()
        } else {
            self.slice1.write().unwrap()
        };

        for z_idx in 0..=cell_count_z {
            let block_z = min_z + (z_idx as i32) * cell_width;
            for y_idx in 0..=cell_count_y {
                let block_y = min_y + (y_idx as i32) * cell_height;
                let ctx = super::context::SinglePointContext::new(block_x, block_y, block_z);
                slice[z_idx][y_idx] = self.wrapped.compute(&ctx);
            }
        }
    }

    /// Select the 8 corners for a specific cell.
    ///
    /// Java naming convention for corners: noiseXYZ where:
    /// - X: 0=slice0, 1=slice1 (X direction)
    /// - Y: 0=cell_y, 1=cell_y+1 (Y direction)
    /// - Z: 0=cell_z, 1=cell_z+1 (Z direction)
    ///
    /// Java array layout: slice[z][y] where j=cell_z, i=cell_y
    pub fn select_cell_yz(&self, cell_y: usize, cell_z: usize) {
        let s0 = self.slice0.read().unwrap();
        let s1 = self.slice1.read().unwrap();

        // Java: noise000 = slice0[j][i]     = slice0[z][y]
        // Java: noise001 = slice0[j+1][i]   = slice0[z+1][y]
        // Java: noise010 = slice0[j][i+1]   = slice0[z][y+1]
        // Java: noise011 = slice0[j+1][i+1] = slice0[z+1][y+1]
        *self.noise_000.write().unwrap() = s0[cell_z][cell_y];
        *self.noise_001.write().unwrap() = s0[cell_z + 1][cell_y];
        *self.noise_010.write().unwrap() = s0[cell_z][cell_y + 1];
        *self.noise_011.write().unwrap() = s0[cell_z + 1][cell_y + 1];
        *self.noise_100.write().unwrap() = s1[cell_z][cell_y];
        *self.noise_101.write().unwrap() = s1[cell_z + 1][cell_y];
        *self.noise_110.write().unwrap() = s1[cell_z][cell_y + 1];
        *self.noise_111.write().unwrap() = s1[cell_z + 1][cell_y + 1];
    }

    /// Update interpolation for Y position.
    ///
    /// Java: valueXZij = lerp(t, noiseij0, noiseij1) for X=0,1 and Z=0,1
    /// This interpolates along Y (the middle digit changes from 0 to 1).
    pub fn update_for_y(&self, t: f64) {
        // Java: valueXZ00 = lerp(d, noise000, noise010) - X=0, Z=0, lerp Y
        *self.value_xz00.write().unwrap() =
            lerp(t, *self.noise_000.read().unwrap(), *self.noise_010.read().unwrap());
        // Java: valueXZ10 = lerp(d, noise100, noise110) - X=1, Z=0, lerp Y
        *self.value_xz10.write().unwrap() =
            lerp(t, *self.noise_100.read().unwrap(), *self.noise_110.read().unwrap());
        // Java: valueXZ01 = lerp(d, noise001, noise011) - X=0, Z=1, lerp Y
        *self.value_xz01.write().unwrap() =
            lerp(t, *self.noise_001.read().unwrap(), *self.noise_011.read().unwrap());
        // Java: valueXZ11 = lerp(d, noise101, noise111) - X=1, Z=1, lerp Y
        *self.value_xz11.write().unwrap() =
            lerp(t, *self.noise_101.read().unwrap(), *self.noise_111.read().unwrap());
    }

    /// Update interpolation for X position.
    pub fn update_for_x(&self, t: f64) {
        *self.value_z0.write().unwrap() =
            lerp(t, *self.value_xz00.read().unwrap(), *self.value_xz10.read().unwrap());
        *self.value_z1.write().unwrap() =
            lerp(t, *self.value_xz01.read().unwrap(), *self.value_xz11.read().unwrap());
    }

    /// Update interpolation for Z position and get final value.
    pub fn update_for_z(&self, t: f64) {
        *self.value.write().unwrap() =
            lerp(t, *self.value_z0.read().unwrap(), *self.value_z1.read().unwrap());
    }

    /// Get the current interpolated value.
    pub fn get_value(&self) -> f64 {
        *self.value.read().unwrap()
    }

    /// Swap slice0 and slice1.
    pub fn swap_slices(&self) {
        let mut s0 = self.slice0.write().unwrap();
        let mut s1 = self.slice1.write().unwrap();
        std::mem::swap(&mut *s0, &mut *s1);
    }
}

impl DensityFunction for NoiseInterpolator {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        // When used as a density function, return the current interpolated value
        self.get_value()
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        let s0 = self.slice0.read().unwrap();
        let cell_count_z = s0.len().saturating_sub(1);
        let cell_count_y = if !s0.is_empty() {
            s0[0].len().saturating_sub(1)
        } else {
            0
        };
        visitor.apply(Arc::new(NoiseInterpolator::new(
            new_wrapped,
            cell_count_z,
            cell_count_y,
        )))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

/// Flat cache for 2D XZ grid.
///
/// Stores an entire XZ grid of values, evaluated once and reused
/// for all Y positions.
pub struct FlatCache {
    /// The wrapped density function.
    wrapped: Arc<dyn DensityFunction>,
    /// Cached values: [size_z][size_x]
    values: RwLock<Vec<Vec<f64>>>,
    /// Whether the cache has been filled.
    filled: RwLock<bool>,
    /// Minimum X coordinate of the cache grid.
    min_x: i32,
    /// Minimum Z coordinate of the cache grid.
    min_z: i32,
}

impl FlatCache {
    /// Create a new flat cache.
    pub fn new(
        wrapped: Arc<dyn DensityFunction>,
        min_x: i32,
        min_z: i32,
        size_x: usize,
        size_z: usize,
    ) -> Self {
        Self {
            wrapped,
            values: RwLock::new(vec![vec![0.0; size_x]; size_z]),
            filled: RwLock::new(false),
            min_x,
            min_z,
        }
    }

    /// Fill the cache with values.
    pub fn fill(&self, y: i32) {
        if *self.filled.read().unwrap() {
            return;
        }

        let mut values = self.values.write().unwrap();
        for (z_idx, row) in values.iter_mut().enumerate() {
            let z = self.min_z + z_idx as i32;
            for (x_idx, val) in row.iter_mut().enumerate() {
                let x = self.min_x + x_idx as i32;
                let ctx = super::context::SinglePointContext::new(x, y, z);
                *val = self.wrapped.compute(&ctx);
            }
        }
        *self.filled.write().unwrap() = true;
    }

    /// Get a cached value.
    pub fn get(&self, x: i32, z: i32) -> f64 {
        let x_idx = (x - self.min_x) as usize;
        let z_idx = (z - self.min_z) as usize;
        let values = self.values.read().unwrap();
        if z_idx < values.len() && x_idx < values[0].len() {
            values[z_idx][x_idx]
        } else {
            0.0
        }
    }
}

impl DensityFunction for FlatCache {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Fill on first access
        if !*self.filled.read().unwrap() {
            self.fill(ctx.block_y());
        }
        self.get(ctx.block_x(), ctx.block_z())
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        let vals = self.values.read().unwrap();
        let size_z = vals.len();
        let size_x = if size_z > 0 { vals[0].len() } else { 0 };
        visitor.apply(Arc::new(FlatCache::new(
            new_wrapped,
            self.min_x,
            self.min_z,
            size_x,
            size_z,
        )))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

/// Cache for all values in a cell.
///
/// Pre-computes density values for all blocks in a cell before iteration.
pub struct CacheAllInCell {
    /// The wrapped density function.
    wrapped: Arc<dyn DensityFunction>,
    /// Cached values: [cell_width * cell_width * cell_height]
    values: RwLock<Vec<f64>>,
    /// Cell dimensions.
    cell_width: i32,
    cell_height: i32,
}

impl CacheAllInCell {
    /// Create a new cell cache.
    pub fn new(wrapped: Arc<dyn DensityFunction>, cell_width: i32, cell_height: i32) -> Self {
        let size = (cell_width * cell_width * cell_height) as usize;
        Self {
            wrapped,
            values: RwLock::new(vec![0.0; size]),
            cell_width,
            cell_height,
        }
    }

    /// Fill the cache for a cell.
    pub fn fill_cell(&self, base_x: i32, base_y: i32, base_z: i32) {
        let mut values = self.values.write().unwrap();
        let mut idx = 0;
        for y in 0..self.cell_height {
            for z in 0..self.cell_width {
                for x in 0..self.cell_width {
                    let ctx = super::context::SinglePointContext::new(
                        base_x + x,
                        base_y + y,
                        base_z + z,
                    );
                    values[idx] = self.wrapped.compute(&ctx);
                    idx += 1;
                }
            }
        }
    }

    /// Get a value from the cache.
    pub fn get(&self, x_in_cell: i32, y_in_cell: i32, z_in_cell: i32) -> f64 {
        let idx = (y_in_cell * self.cell_width * self.cell_width
            + z_in_cell * self.cell_width
            + x_in_cell) as usize;
        self.values.read().unwrap()[idx]
    }
}

impl DensityFunction for CacheAllInCell {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        // This should be used via get() after fill_cell()
        // Fallback: compute directly
        0.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(CacheAllInCell::new(
            new_wrapped,
            self.cell_width,
            self.cell_height,
        )))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::math::Constant;

    #[test]
    fn test_cache_2d() {
        let inner = Arc::new(Constant::new(42.0));
        let cache = Cache2D::new(inner);

        let ctx1 = super::super::context::SinglePointContext::new(10, 0, 20);
        assert_eq!(cache.compute(&ctx1), 42.0);

        // Same XZ should return cached
        let ctx2 = super::super::context::SinglePointContext::new(10, 64, 20);
        assert_eq!(cache.compute(&ctx2), 42.0);
    }

    #[test]
    fn test_noise_interpolator_lerp() {
        // Simple test: corners all same value should interpolate to same value
        let inner = Arc::new(Constant::new(5.0));
        let interp = NoiseInterpolator::new(inner, 4, 24);

        // Manually set corner values
        *interp.noise_000.write().unwrap() = 0.0;
        *interp.noise_001.write().unwrap() = 0.0;
        *interp.noise_010.write().unwrap() = 0.0;
        *interp.noise_011.write().unwrap() = 0.0;
        *interp.noise_100.write().unwrap() = 8.0;
        *interp.noise_101.write().unwrap() = 8.0;
        *interp.noise_110.write().unwrap() = 8.0;
        *interp.noise_111.write().unwrap() = 8.0;

        // At t_y=0, t_x=0.5, t_z=0 should give 4.0
        interp.update_for_y(0.0);
        interp.update_for_x(0.5);
        interp.update_for_z(0.0);

        assert!((interp.get_value() - 4.0).abs() < 0.001);
    }
}
