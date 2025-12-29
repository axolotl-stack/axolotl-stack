//! Blend density functions for biome/chunk transitions.
//!
//! These functions handle smooth blending between old and new terrain
//! during world upgrades and at chunk borders.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use std::sync::Arc;

/// Blend alpha density function.
///
/// Returns the blending factor (0-1) for transitioning between old and new terrain.
/// Used during world upgrades when old chunks need to blend with new generation.
#[derive(Debug, Clone)]
pub struct BlendAlpha;

impl BlendAlpha {
    /// Create a new blend alpha function.
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlendAlpha {
    fn default() -> Self {
        Self::new()
    }
}

impl DensityFunction for BlendAlpha {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        // Default: no blending needed (return 1.0 = use new terrain)
        // In actual implementation, this would query the blend data
        1.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        0.0
    }

    fn max_value(&self) -> f64 {
        1.0
    }
}

/// Blend offset density function.
///
/// Returns the Y offset for blending old terrain height with new terrain.
#[derive(Debug, Clone)]
pub struct BlendOffset;

impl BlendOffset {
    /// Create a new blend offset function.
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlendOffset {
    fn default() -> Self {
        Self::new()
    }
}

impl DensityFunction for BlendOffset {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        // Default: no offset
        0.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        // Java uses: Blender.BLENDING_OFFSET_RANGE
        -8.0
    }

    fn max_value(&self) -> f64 {
        8.0
    }
}

/// Blend density function.
///
/// Blends an input density with old terrain density based on blend alpha.
#[derive(Clone)]
pub struct BlendDensity {
    /// The new terrain density to blend.
    pub input: Arc<dyn DensityFunction>,
}

impl BlendDensity {
    /// Create a new blend density function.
    pub fn new(input: Arc<dyn DensityFunction>) -> Self {
        Self { input }
    }
}

impl DensityFunction for BlendDensity {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Default: just return input (no blending needed)
        // In actual implementation: lerp(blendAlpha, oldDensity, input)
        self.input.compute(ctx)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        visitor.apply(Arc::new(BlendDensity::new(new_input)))
    }

    fn min_value(&self) -> f64 {
        // Conservative: could be blended with anything
        -64.0
    }

    fn max_value(&self) -> f64 {
        64.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::context::SinglePointContext;
    use crate::world::generator::density::math::Constant;

    #[test]
    fn test_blend_alpha_default() {
        let blend = BlendAlpha::new();
        let ctx = SinglePointContext::new(0, 64, 0);
        assert_eq!(blend.compute(&ctx), 1.0);
    }

    #[test]
    fn test_blend_offset_default() {
        let blend = BlendOffset::new();
        let ctx = SinglePointContext::new(0, 64, 0);
        assert_eq!(blend.compute(&ctx), 0.0);
    }

    #[test]
    fn test_blend_density_passthrough() {
        let input = Arc::new(Constant::new(42.0));
        let blend = BlendDensity::new(input);
        let ctx = SinglePointContext::new(0, 64, 0);
        assert_eq!(blend.compute(&ctx), 42.0);
    }
}
