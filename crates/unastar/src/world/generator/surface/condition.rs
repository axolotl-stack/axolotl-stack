//! Surface rule conditions.
//!
//! Conditions determine when surface rules should be applied.
//! Each condition evaluates to true or false based on the current
//! [`SurfaceContext`].

use super::context::{CaveSurface, SurfaceContext, VerticalAnchor};
use crate::world::generator::constants::Biome;
use crate::world::generator::noise::DoublePerlinNoise;
use crate::world::generator::xoroshiro::Xoroshiro128;

/// Trait for surface rule conditions.
///
/// Conditions are evaluated for each block position to determine
/// whether a rule should be applied.
pub trait Condition: Send + Sync {
    /// Test whether the condition is met for the given context.
    fn test(&self, ctx: &SurfaceContext) -> bool;
}

/// Trait for lazy conditions that cache their result.
///
/// Lazy conditions compute their value once per update cycle
/// and cache the result.
pub trait LazyCondition: Condition {
    /// Compute the condition value.
    fn compute(&self, ctx: &SurfaceContext) -> bool;

    /// Get the last update counter.
    fn last_update(&self) -> u64;
}

/// Stone depth check (distance from surface).
///
/// Checks whether the stone depth (distance from air/water to this block)
/// is within a threshold.
#[derive(Debug, Clone)]
pub struct StoneDepthCheck {
    /// Base offset for the threshold.
    pub offset: i32,
    /// Whether to add surface depth to the threshold.
    pub add_surface_depth: bool,
    /// Secondary depth range for variation.
    pub secondary_depth_range: i32,
    /// Which surface to check (floor or ceiling).
    pub surface_type: CaveSurface,
}

impl Condition for StoneDepthCheck {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        let stone_depth = match self.surface_type {
            CaveSurface::Floor => ctx.stone_depth_above,
            CaveSurface::Ceiling => ctx.stone_depth_below,
        };

        let mut threshold = 1 + self.offset;
        if self.add_surface_depth {
            threshold += ctx.surface_depth;
        }
        if self.secondary_depth_range > 0 {
            let secondary = ctx.surface_secondary;
            threshold += lerp(secondary, 0.0, self.secondary_depth_range as f64) as i32;
        }

        stone_depth <= threshold
    }
}

impl StoneDepthCheck {
    /// Create a simple floor check with the given offset.
    pub fn floor(offset: i32) -> Self {
        Self {
            offset,
            add_surface_depth: false,
            secondary_depth_range: 0,
            surface_type: CaveSurface::Floor,
        }
    }

    /// Create a floor check with surface depth.
    pub fn floor_with_depth(offset: i32, add_surface_depth: bool) -> Self {
        Self {
            offset,
            add_surface_depth,
            secondary_depth_range: 0,
            surface_type: CaveSurface::Floor,
        }
    }
}

/// Y coordinate check.
///
/// Checks whether the block Y coordinate (optionally modified)
/// is at or above a threshold.
#[derive(Debug, Clone)]
pub struct YCheck {
    /// The anchor Y value to compare against.
    pub anchor: VerticalAnchor,
    /// Multiplier for surface depth to add to Y.
    pub surface_depth_multiplier: i32,
    /// Whether to add stone depth to Y.
    pub add_stone_depth: bool,
}

impl Condition for YCheck {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        let target_y = self.anchor.resolve(ctx.min_y, ctx.max_y);
        let mut block_y = ctx.block_y;

        if self.add_stone_depth {
            block_y += ctx.stone_depth_above;
        }

        block_y >= target_y + ctx.surface_depth * self.surface_depth_multiplier
    }
}

impl YCheck {
    /// Create a simple Y check at an absolute height.
    pub fn at(y: i32) -> Self {
        Self {
            anchor: VerticalAnchor::Absolute(y),
            surface_depth_multiplier: 0,
            add_stone_depth: false,
        }
    }

    /// Create a Y check above the world bottom.
    pub fn above_bottom(offset: i32) -> Self {
        Self {
            anchor: VerticalAnchor::AboveBottom(offset),
            surface_depth_multiplier: 0,
            add_stone_depth: false,
        }
    }
}

/// Water level check.
///
/// Checks whether the block is at or above the water level
/// (optionally modified by offsets).
#[derive(Debug, Clone)]
pub struct WaterCheck {
    /// Offset from water level.
    pub offset: i32,
    /// Multiplier for surface depth.
    pub surface_depth_multiplier: i32,
    /// Whether to add stone depth to Y.
    pub add_stone_depth: bool,
}

impl Condition for WaterCheck {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        if ctx.water_height == i32::MIN {
            return true; // No water nearby
        }

        let mut block_y = ctx.block_y;
        if self.add_stone_depth {
            block_y += ctx.stone_depth_above;
        }

        block_y >= ctx.water_height + self.offset + ctx.surface_depth * self.surface_depth_multiplier
    }
}

impl WaterCheck {
    /// Create a simple water check with an offset.
    pub fn new(offset: i32) -> Self {
        Self {
            offset,
            surface_depth_multiplier: 0,
            add_stone_depth: false,
        }
    }
}

/// Biome check.
///
/// Checks whether the current biome matches any of the specified biomes.
#[derive(Debug, Clone)]
pub struct BiomeCheck {
    /// List of biomes to match.
    pub biomes: Vec<Biome>,
}

impl Condition for BiomeCheck {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        self.biomes.contains(&ctx.biome)
    }
}

impl BiomeCheck {
    /// Create a biome check for a single biome.
    pub fn single(biome: Biome) -> Self {
        Self {
            biomes: vec![biome],
        }
    }

    /// Create a biome check for multiple biomes.
    pub fn multiple(biomes: Vec<Biome>) -> Self {
        Self { biomes }
    }
}

/// Noise threshold check.
///
/// Checks whether a noise value at the current position is within a range.
#[derive(Debug)]
pub struct NoiseThreshold {
    /// The noise function to sample.
    pub noise: DoublePerlinNoise,
    /// Minimum threshold (inclusive).
    pub min_threshold: f64,
    /// Maximum threshold (inclusive).
    pub max_threshold: f64,
}

impl Condition for NoiseThreshold {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        let value = self.noise.sample(ctx.block_x as f64, 0.0, ctx.block_z as f64);
        value >= self.min_threshold && value <= self.max_threshold
    }
}

/// Vertical gradient (probabilistic).
///
/// Returns true with a probability that varies based on Y coordinate.
#[derive(Debug)]
pub struct VerticalGradient {
    /// Y at and below which the condition is always true.
    pub true_at_and_below: i32,
    /// Y at and above which the condition is always false.
    pub false_at_and_above: i32,
    /// Seed for deterministic randomness.
    pub seed: i64,
}

impl Condition for VerticalGradient {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        let y = ctx.block_y;

        if y <= self.true_at_and_below {
            return true;
        }
        if y >= self.false_at_and_above {
            return false;
        }

        let range = (self.false_at_and_above - self.true_at_and_below) as f64;
        let prob = (self.false_at_and_above - y) as f64 / range;

        // Create positional random
        let pos_seed = self
            .seed
            .wrapping_add(ctx.block_x as i64 * 341873128712)
            .wrapping_add(ctx.block_y as i64 * 132897987541)
            .wrapping_add(ctx.block_z as i64 * 1664525);
        let mut rng = Xoroshiro128::from_seed(pos_seed);
        let random = rng.next_float();

        random < prob as f32
    }
}

impl VerticalGradient {
    /// Create a new vertical gradient condition.
    pub fn new(true_at_and_below: i32, false_at_and_above: i32, seed: i64) -> Self {
        Self {
            true_at_and_below,
            false_at_and_above,
            seed,
        }
    }
}

/// Steep terrain check.
///
/// Returns true if the current position is on steep terrain.
#[derive(Debug, Clone, Copy, Default)]
pub struct Steep;

impl Condition for Steep {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        ctx.steep
    }
}

/// Hole check (surface depth <= 0).
///
/// Returns true if the surface depth is 0 or negative (a hole in terrain).
#[derive(Debug, Clone, Copy, Default)]
pub struct Hole;

impl Condition for Hole {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        ctx.surface_depth <= 0
    }
}

/// Negation condition.
///
/// Returns the opposite of the inner condition.
pub struct Not {
    /// The condition to negate.
    pub inner: Box<dyn Condition>,
}

impl std::fmt::Debug for Not {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Not").finish()
    }
}

impl Condition for Not {
    fn test(&self, ctx: &SurfaceContext) -> bool {
        !self.inner.test(ctx)
    }
}

impl Not {
    /// Create a new negation of a condition.
    pub fn new(inner: Box<dyn Condition>) -> Self {
        Self { inner }
    }
}

/// Linear interpolation helper.
#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stone_depth_check_floor() {
        let condition = StoneDepthCheck::floor(0);

        let mut ctx = SurfaceContext::default();
        ctx.stone_depth_above = 1;
        assert!(condition.test(&ctx), "stone_depth_above=1 should pass offset=0");

        ctx.stone_depth_above = 2;
        assert!(!condition.test(&ctx), "stone_depth_above=2 should fail offset=0");
    }

    #[test]
    fn test_stone_depth_check_with_surface_depth() {
        let condition = StoneDepthCheck {
            offset: 0,
            add_surface_depth: true,
            secondary_depth_range: 0,
            surface_type: CaveSurface::Floor,
        };

        let mut ctx = SurfaceContext::default();
        ctx.surface_depth = 3;
        ctx.stone_depth_above = 4;
        assert!(condition.test(&ctx), "stone_depth=4, surface_depth=3 should pass");

        ctx.stone_depth_above = 5;
        assert!(!condition.test(&ctx), "stone_depth=5, surface_depth=3 should fail");
    }

    #[test]
    fn test_y_check() {
        let condition = YCheck::at(63);

        let mut ctx = SurfaceContext::default();
        ctx.block_y = 64;
        assert!(condition.test(&ctx), "Y=64 should pass Y>=63");

        ctx.block_y = 62;
        assert!(!condition.test(&ctx), "Y=62 should fail Y>=63");
    }

    #[test]
    fn test_water_check() {
        let condition = WaterCheck::new(0);

        let mut ctx = SurfaceContext::default();
        ctx.water_height = 63;
        ctx.block_y = 64;
        assert!(condition.test(&ctx), "Y=64 should be above water at 63");

        ctx.block_y = 62;
        assert!(!condition.test(&ctx), "Y=62 should be below water at 63");
    }

    #[test]
    fn test_biome_check() {
        let condition = BiomeCheck::single(Biome::Desert);

        let mut ctx = SurfaceContext::default();
        ctx.biome = Biome::Desert;
        assert!(condition.test(&ctx), "Desert should match");

        ctx.biome = Biome::Plains;
        assert!(!condition.test(&ctx), "Plains should not match");
    }

    #[test]
    fn test_steep() {
        let condition = Steep;

        let mut ctx = SurfaceContext::default();
        ctx.steep = true;
        assert!(condition.test(&ctx));

        ctx.steep = false;
        assert!(!condition.test(&ctx));
    }

    #[test]
    fn test_hole() {
        let condition = Hole;

        let mut ctx = SurfaceContext::default();
        ctx.surface_depth = 0;
        assert!(condition.test(&ctx));

        ctx.surface_depth = 1;
        assert!(!condition.test(&ctx));
    }

    #[test]
    fn test_not() {
        let condition = Not::new(Box::new(Steep));

        let mut ctx = SurfaceContext::default();
        ctx.steep = true;
        assert!(!condition.test(&ctx));

        ctx.steep = false;
        assert!(condition.test(&ctx));
    }

    #[test]
    fn test_vertical_gradient() {
        let condition = VerticalGradient::new(-60, -55, 12345);

        let mut ctx = SurfaceContext::default();

        // Always true below threshold
        ctx.block_y = -61;
        assert!(condition.test(&ctx));

        // Always false above threshold
        ctx.block_y = -54;
        assert!(!condition.test(&ctx));
    }
}
