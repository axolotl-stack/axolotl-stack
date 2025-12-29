//! Marker types for cache implementations.
//!
//! These markers are used during density function construction to indicate
//! where caching should be applied. During the wiring phase (when `wrap()` is
//! called on a NoiseChunk), these markers are replaced with actual cache
//! implementations.

use super::context::{ContextProvider, FunctionContext};
use super::function::{DensityFunction, Visitor};
use std::sync::Arc;

/// Marker for interpolated caching.
///
/// Functions wrapped in this marker will be evaluated at cell corners
/// and trilinearly interpolated for points within the cell.
#[derive(Clone)]
pub struct Interpolated {
    /// The wrapped density function.
    pub wrapped: Arc<dyn DensityFunction>,
}

impl Interpolated {
    /// Create a new interpolation marker.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self { wrapped }
    }
}

impl DensityFunction for Interpolated {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Before wiring, just pass through to wrapped function
        self.wrapped.compute(ctx)
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        self.wrapped.fill_array(values, provider)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(Interpolated::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }

}

/// Marker for flat (2D XZ) caching.
///
/// Functions wrapped in this marker are evaluated once per XZ column
/// and reused for all Y values in that column.
#[derive(Clone)]
pub struct FlatCacheMarker {
    /// The wrapped density function.
    pub wrapped: Arc<dyn DensityFunction>,
}

impl FlatCacheMarker {
    /// Create a new flat cache marker.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self { wrapped }
    }
}

impl DensityFunction for FlatCacheMarker {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Before wiring, just pass through to wrapped function
        self.wrapped.compute(ctx)
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        self.wrapped.fill_array(values, provider)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(FlatCacheMarker::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }

}

/// Marker for 2D single-point cache.
///
/// Caches the last evaluated XZ position and returns the cached value
/// if the same position is queried again.
#[derive(Clone)]
pub struct Cache2DMarker {
    /// The wrapped density function.
    pub wrapped: Arc<dyn DensityFunction>,
}

impl Cache2DMarker {
    /// Create a new 2D cache marker.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self { wrapped }
    }
}

impl DensityFunction for Cache2DMarker {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Before wiring, just pass through to wrapped function
        self.wrapped.compute(ctx)
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        self.wrapped.fill_array(values, provider)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(Cache2DMarker::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}

/// Marker for once-per-step caching.
///
/// Caches the result for a single interpolation step, invalidating
/// when the interpolation counter advances.
#[derive(Clone)]
pub struct CacheOnceMarker {
    /// The wrapped density function.
    pub wrapped: Arc<dyn DensityFunction>,
}

impl CacheOnceMarker {
    /// Create a new cache-once marker.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self { wrapped }
    }
}

impl DensityFunction for CacheOnceMarker {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Before wiring, just pass through to wrapped function
        self.wrapped.compute(ctx)
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        self.wrapped.fill_array(values, provider)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(CacheOnceMarker::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }

}

/// Marker for caching all values in a cell.
///
/// Pre-computes all density values for all blocks in a cell
/// before interpolation begins.
#[derive(Clone)]
pub struct CacheAllInCellMarker {
    /// The wrapped density function.
    pub wrapped: Arc<dyn DensityFunction>,
}

impl CacheAllInCellMarker {
    /// Create a new cache-all-in-cell marker.
    pub fn new(wrapped: Arc<dyn DensityFunction>) -> Self {
        Self { wrapped }
    }
}

impl DensityFunction for CacheAllInCellMarker {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Before wiring, just pass through to wrapped function
        self.wrapped.compute(ctx)
    }

    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        self.wrapped.fill_array(values, provider)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_wrapped = self.wrapped.map_all(visitor);
        visitor.apply(Arc::new(CacheAllInCellMarker::new(new_wrapped)))
    }

    fn min_value(&self) -> f64 {
        self.wrapped.min_value()
    }

    fn max_value(&self) -> f64 {
        self.wrapped.max_value()
    }
}
