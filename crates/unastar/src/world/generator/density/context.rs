//! Evaluation context for density functions.
//!
//! This module provides context types for density function evaluation.
//! With the new enum-based DensityFunction system, we use the generated
//! FunctionContext struct for single-point evaluation.

use unastar_noise::FunctionContext;

/// Provider for batch context iteration.
///
/// Used for `fill_array` operations where multiple positions
/// need to be evaluated in sequence.
pub trait ContextProvider: Send + Sync {
    /// Get a context for the given index in the batch.
    fn for_index(&self, index: usize) -> FunctionContext;
}

/// Simple single-point context.
///
/// Convenience wrapper around FunctionContext for evaluating density at a specific coordinate.
#[derive(Debug, Clone, Copy)]
pub struct SinglePointContext {
    inner: FunctionContext,
}

impl SinglePointContext {
    /// Create a new single point context.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self {
            inner: FunctionContext::new(x, y, z),
        }
    }

    /// Get the inner FunctionContext.
    pub fn as_context(&self) -> &FunctionContext {
        &self.inner
    }

    /// Get block X coordinate.
    pub fn block_x(&self) -> i32 {
        self.inner.block_x
    }

    /// Get block Y coordinate.
    pub fn block_y(&self) -> i32 {
        self.inner.block_y
    }

    /// Get block Z coordinate.
    pub fn block_z(&self) -> i32 {
        self.inner.block_z
    }
}

impl From<SinglePointContext> for FunctionContext {
    fn from(ctx: SinglePointContext) -> Self {
        ctx.inner
    }
}

impl AsRef<FunctionContext> for SinglePointContext {
    fn as_ref(&self) -> &FunctionContext {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_point_context() {
        let ctx = SinglePointContext::new(10, 64, -20);
        assert_eq!(ctx.block_x(), 10);
        assert_eq!(ctx.block_y(), 64);
        assert_eq!(ctx.block_z(), -20);
    }
}
