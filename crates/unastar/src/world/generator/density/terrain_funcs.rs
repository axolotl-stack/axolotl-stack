//! Terrain-specific density functions.
//!
//! These functions handle special terrain generation cases like
//! vertical sliding at world edges, structure integration, and End islands.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use super::math::lerp;
use std::sync::Arc;

/// Slide density function.
///
/// Applies vertical falloff at the top and bottom of the world,
/// smoothly transitioning terrain to air (top) or solid (bottom).
#[derive(Clone)]
pub struct Slide {
    /// The input density to slide.
    pub input: Arc<dyn DensityFunction>,
    /// Minimum Y coordinate of the world.
    pub min_y: i32,
    /// Maximum Y coordinate of the world.
    pub max_y: i32,
    /// Y offset from top where sliding begins.
    pub top_slide_offset: i32,
    /// Size of the top slide region in blocks.
    pub top_slide_size: i32,
    /// Target value at top of world (usually negative = air).
    pub top_slide_target: f64,
    /// Y offset from bottom where sliding begins.
    pub bottom_slide_offset: i32,
    /// Size of the bottom slide region in blocks.
    pub bottom_slide_size: i32,
    /// Target value at bottom of world (usually positive = solid).
    pub bottom_slide_target: f64,
}

impl Slide {
    /// Create a new slide function with default overworld settings.
    pub fn new(input: Arc<dyn DensityFunction>) -> Self {
        Self {
            input,
            min_y: -64,
            max_y: 320,
            top_slide_offset: -8,
            top_slide_size: 16,
            top_slide_target: -0.078125,
            bottom_slide_offset: 0,
            bottom_slide_size: 8,
            bottom_slide_target: 0.1171875,
        }
    }

    /// Create a slide function with custom settings.
    pub fn with_settings(
        input: Arc<dyn DensityFunction>,
        min_y: i32,
        max_y: i32,
        top_slide_offset: i32,
        top_slide_size: i32,
        top_slide_target: f64,
        bottom_slide_offset: i32,
        bottom_slide_size: i32,
        bottom_slide_target: f64,
    ) -> Self {
        Self {
            input,
            min_y,
            max_y,
            top_slide_offset,
            top_slide_size,
            top_slide_target,
            bottom_slide_offset,
            bottom_slide_size,
            bottom_slide_target,
        }
    }
}

impl DensityFunction for Slide {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let y = ctx.block_y();
        let density = self.input.compute(ctx);

        // Apply bottom slide (near min_y)
        let bottom_start = self.min_y + self.bottom_slide_offset;
        if y < bottom_start + self.bottom_slide_size && self.bottom_slide_size > 0 {
            let t = ((bottom_start + self.bottom_slide_size - y) as f64)
                / (self.bottom_slide_size as f64);
            let t = t.clamp(0.0, 1.0);
            return lerp(t, density, self.bottom_slide_target);
        }

        // Apply top slide (near max_y)
        let top_start = self.max_y + self.top_slide_offset;
        if y > top_start - self.top_slide_size && self.top_slide_size > 0 {
            let t = ((y - (top_start - self.top_slide_size)) as f64) / (self.top_slide_size as f64);
            let t = t.clamp(0.0, 1.0);
            return lerp(t, density, self.top_slide_target);
        }

        density
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        visitor.apply(Arc::new(Slide {
            input: new_input,
            min_y: self.min_y,
            max_y: self.max_y,
            top_slide_offset: self.top_slide_offset,
            top_slide_size: self.top_slide_size,
            top_slide_target: self.top_slide_target,
            bottom_slide_offset: self.bottom_slide_offset,
            bottom_slide_size: self.bottom_slide_size,
            bottom_slide_target: self.bottom_slide_target,
        }))
    }

    fn min_value(&self) -> f64 {
        self.input
            .min_value()
            .min(self.top_slide_target)
            .min(self.bottom_slide_target)
    }

    fn max_value(&self) -> f64 {
        self.input
            .max_value()
            .max(self.top_slide_target)
            .max(self.bottom_slide_target)
    }
}

/// Beardifier density function.
///
/// Modifies terrain density around structures to create smooth transitions.
/// "Beard" refers to the terrain modification under structures like villages.
#[derive(Clone)]
pub struct Beardifier {
    // In a full implementation, this would hold references to structure data
    // For now, we provide a no-op implementation that can be extended
}

impl Beardifier {
    /// Create a new beardifier.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Beardifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DensityFunction for Beardifier {
    fn compute(&self, _ctx: &dyn FunctionContext) -> f64 {
        // Default: no structure modification
        // In full implementation: query nearby structures and return density modification
        0.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        // Structure modifications can significantly increase density
        -1.0
    }

    fn max_value(&self) -> f64 {
        1.0
    }
}

/// End Islands density function.
///
/// Generates the distinctive floating island terrain of the End dimension.
#[derive(Debug, Clone)]
pub struct EndIslands {
    /// Seed for the End island noise.
    pub seed: i64,
}

impl EndIslands {
    /// Create a new End islands function.
    pub fn new(seed: i64) -> Self {
        Self { seed }
    }

    /// Calculate the island height contribution at a given XZ position.
    fn get_height(&self, x: i32, z: i32) -> f64 {
        // Simplified End island algorithm
        // The real algorithm uses SimplexNoise sampled at grid positions

        // Convert to grid coordinates (End uses 8-block grid)
        let grid_x = (x / 8) as f64;
        let grid_z = (z / 8) as f64;

        // Distance from center (0, 0)
        let dist_sq = grid_x * grid_x + grid_z * grid_z;

        // Main island is at center, outer islands are rings
        if dist_sq < 64.0 {
            // Main island area
            100.0 - (dist_sq * 0.5).sqrt() * 8.0
        } else if dist_sq < 1024.0 {
            // Transition zone - mostly empty
            -100.0
        } else {
            // Outer islands - use pseudo-random based on position
            let hash = self.hash_position(x / 64, z / 64);
            if hash % 13 == 0 {
                // Island here
                let local_x = (x % 64) as f64 - 32.0;
                let local_z = (z % 64) as f64 - 32.0;
                let local_dist = (local_x * local_x + local_z * local_z).sqrt();
                (40.0 - local_dist).max(-100.0)
            } else {
                -100.0
            }
        }
    }

    fn hash_position(&self, x: i32, z: i32) -> u64 {
        let mut hash = self.seed as u64;
        hash = hash.wrapping_mul(0x5851F42D4C957F2D);
        hash = hash.wrapping_add(x as u64);
        hash = hash.wrapping_mul(0x5851F42D4C957F2D);
        hash = hash.wrapping_add(z as u64);
        hash
    }
}

impl DensityFunction for EndIslands {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let x = ctx.block_x();
        let y = ctx.block_y();
        let z = ctx.block_z();

        // End islands are centered around Y=64
        let height = self.get_height(x, z);
        let y_offset = (y - 64) as f64;

        // Density decreases as we go away from the island surface
        height - y_offset.abs()
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -200.0
    }

    fn max_value(&self) -> f64 {
        100.0
    }
}

/// Old blended noise for legacy terrain compatibility.
///
/// Used when blending between old and new terrain generation algorithms.
#[derive(Clone)]
pub struct OldBlendedNoise {
    /// Minimum Y for blending.
    pub min_y: i32,
    /// Maximum Y for blending.
    pub max_y: i32,
    /// XZ scale for noise.
    pub xz_scale: f64,
    /// Y scale for noise.
    pub y_scale: f64,
    /// XZ factor.
    pub xz_factor: f64,
    /// Y factor.
    pub y_factor: f64,
    /// Smear scale for Y.
    pub smear_scale: f64,
}

impl OldBlendedNoise {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            min_y: -64,
            max_y: 320,
            xz_scale: 684.412,
            y_scale: 684.412,
            xz_factor: 80.0,
            y_factor: 160.0,
            smear_scale: 8.0,
        }
    }
}

impl Default for OldBlendedNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl DensityFunction for OldBlendedNoise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        // Simplified: in full implementation this would sample legacy noise
        let y = ctx.block_y();
        let normalized_y = (y - self.min_y) as f64 / (self.max_y - self.min_y) as f64;

        // Basic falloff: more solid at bottom, more air at top
        0.5 - normalized_y
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -1.0
    }

    fn max_value(&self) -> f64 {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::context::SinglePointContext;
    use crate::world::generator::density::math::Constant;

    #[test]
    fn test_slide_passthrough() {
        // Test that slide passes through values in the middle of the world
        let input = Arc::new(Constant::new(0.5));
        let slide = Slide::new(input);

        let ctx = SinglePointContext::new(0, 64, 0);
        assert!((slide.compute(&ctx) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_slide_top() {
        // Test that slide modifies values near the top
        let input = Arc::new(Constant::new(0.5));
        let slide = Slide::new(input);

        let ctx = SinglePointContext::new(0, 315, 0); // Near top
        let value = slide.compute(&ctx);
        // Should be sliding towards top_slide_target
        assert!(value < 0.5);
    }

    #[test]
    fn test_slide_bottom() {
        // Test that slide modifies values near the bottom
        let input = Arc::new(Constant::new(0.5));
        let slide = Slide::new(input);

        let ctx = SinglePointContext::new(0, -60, 0); // Near bottom
        let value = slide.compute(&ctx);
        // Should be sliding towards bottom_slide_target (0.1171875)
        // At Y=-60 with bottom_slide_offset=0 and size=8, we're in the slide region
        // Value should be between input (0.5) and target (0.1171875)
        assert!(value >= 0.1 && value <= 0.6);
    }

    #[test]
    fn test_beardifier_default() {
        let beard = Beardifier::new();
        let ctx = SinglePointContext::new(0, 64, 0);
        assert_eq!(beard.compute(&ctx), 0.0);
    }

    #[test]
    fn test_end_islands_center() {
        let end = EndIslands::new(12345);
        let ctx = SinglePointContext::new(0, 64, 0);
        // Center of End should have positive density (solid)
        assert!(end.compute(&ctx) > 0.0);
    }

    #[test]
    fn test_end_islands_far() {
        let end = EndIslands::new(12345);
        let ctx = SinglePointContext::new(500, 64, 500);
        // Far from center, in the void ring
        let density = end.compute(&ctx);
        // Could be positive (outer island) or negative (void)
        assert!(density.is_finite());
    }
}
