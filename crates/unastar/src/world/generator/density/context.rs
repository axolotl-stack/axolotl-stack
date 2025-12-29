//! Evaluation context for density functions.

use std::sync::Arc;

/// Context for density function evaluation.
///
/// Provides the current block coordinates being evaluated.
pub trait FunctionContext: Send + Sync {
    /// Get the X coordinate of the current block.
    fn block_x(&self) -> i32;

    /// Get the Y coordinate of the current block.
    fn block_y(&self) -> i32;

    /// Get the Z coordinate of the current block.
    fn block_z(&self) -> i32;
}

/// Provider for batch context iteration.
///
/// Used for `fill_array` operations where multiple positions
/// need to be evaluated in sequence.
pub trait ContextProvider: Send + Sync {
    /// Get a context for the given index in the batch.
    fn for_index(&self, index: usize) -> Arc<dyn FunctionContext>;

    /// Fill all values directly using the provided function.
    ///
    /// This can be more efficient than individual `for_index` calls
    /// when the provider can optimize batch access.
    fn fill_all_directly(
        &self,
        values: &mut [f64],
        func: &dyn super::function::DensityFunction,
    ) {
        for (i, value) in values.iter_mut().enumerate() {
            *value = func.compute(self.for_index(i).as_ref());
        }
    }
}

/// Simple single-point context.
///
/// Used for evaluating density at a specific coordinate.
#[derive(Debug, Clone, Copy)]
pub struct SinglePointContext {
    /// X coordinate
    pub x: i32,
    /// Y coordinate
    pub y: i32,
    /// Z coordinate
    pub z: i32,
}

impl SinglePointContext {
    /// Create a new single point context.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

impl FunctionContext for SinglePointContext {
    fn block_x(&self) -> i32 {
        self.x
    }

    fn block_y(&self) -> i32 {
        self.y
    }

    fn block_z(&self) -> i32 {
        self.z
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
