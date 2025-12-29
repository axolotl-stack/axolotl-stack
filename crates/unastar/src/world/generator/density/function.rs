//! Core density function trait definitions.

use super::context::{ContextProvider, FunctionContext};
use std::sync::Arc;

/// Core trait for density functions.
///
/// Density > 0 = solid block, density <= 0 = air/fluid.
/// This trait is the foundation of the composable terrain generation system.
pub trait DensityFunction: Send + Sync {
    /// Compute density at a single point.
    fn compute(&self, ctx: &dyn FunctionContext) -> f64;

    /// Fill array with density values (for batch processing).
    ///
    /// Default implementation iterates `compute()` for each index.
    fn fill_array(&self, values: &mut [f64], provider: &dyn ContextProvider) {
        for (i, value) in values.iter_mut().enumerate() {
            *value = self.compute(provider.for_index(i).as_ref());
        }
    }

    /// Transform this function using a visitor (for caching/wiring).
    ///
    /// Returns a new density function that may have been transformed
    /// by the visitor (e.g., wrapped in a cache).
    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction>;

    /// Minimum possible value this function can return.
    fn min_value(&self) -> f64;

    /// Maximum possible value this function can return.
    fn max_value(&self) -> f64;
}

/// Visitor for transforming density function graphs.
///
/// Used during the wiring phase to replace marker functions with
/// actual cache implementations.
pub trait Visitor: Send + Sync {
    /// Apply transformation to a density function.
    fn apply(&self, func: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction>;
}

/// Identity visitor that returns functions unchanged.
pub struct IdentityVisitor;

impl Visitor for IdentityVisitor {
    fn apply(&self, func: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        func
    }
}
