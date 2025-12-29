//! Terrain-specific density functions.
//!
//! These functions handle special terrain generation cases like
//! vertical sliding at world edges, structure integration, and End islands.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use super::math::lerp;
use crate::world::generator::noise::PerlinNoise;
use crate::world::generator::xoroshiro::Xoroshiro128;
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

/// Blended noise for 3D terrain generation.
///
/// This matches Java's BlendedNoise class. It uses three noise instances
/// (min_limit, max_limit, main) blended together to create terrain density.
///
/// The main noise selects between min and max limit noises, creating
/// varied terrain with both steep and gentle slopes.
#[derive(Clone)]
pub struct BlendedNoise {
    /// Minimum limit noise (16 octaves: -15 to 0)
    min_limit_noise: LegacyOctaveNoise,
    /// Maximum limit noise (16 octaves: -15 to 0)
    max_limit_noise: LegacyOctaveNoise,
    /// Main/selector noise (8 octaves: -7 to 0)
    main_noise: LegacyOctaveNoise,
    /// XZ scale (typically 1.0)
    xz_scale: f64,
    /// Y scale (typically 1.0)
    y_scale: f64,
    /// XZ factor for main noise division (typically 80.0)
    xz_factor: f64,
    /// Y factor for main noise division (typically 160.0)
    y_factor: f64,
    /// Smear scale multiplier (typically 8.0)
    smear_scale_multiplier: f64,
    /// Precomputed: 684.412 * xz_scale
    xz_multiplier: f64,
    /// Precomputed: 684.412 * y_scale
    y_multiplier: f64,
    /// Maximum possible output value
    max_value: f64,
}

/// Legacy octave noise for BlendedNoise.
///
/// This is a simplified octave noise that matches Java's legacy initialization
/// used by BlendedNoise (non-positional random source).
#[derive(Clone)]
struct LegacyOctaveNoise {
    /// Individual noise octaves (may have None for skipped octaves)
    octaves: Vec<Option<PerlinNoise>>,
    /// First octave index (negative)
    first_octave: i32,
    /// Number of octaves
    num_octaves: usize,
}

impl LegacyOctaveNoise {
    /// Create legacy octave noise matching Java's createLegacyForBlendedNoise.
    fn new(rng: &mut Xoroshiro128, first_octave: i32, num_octaves: usize) -> Self {
        let mut octaves = Vec::with_capacity(num_octaves);

        // The legacy path creates octaves differently:
        // First creates one at index -first_octave, then fills backwards

        // Create initial noise at the "anchor" position
        let anchor_idx = (-first_octave) as usize;
        let anchor_noise = PerlinNoise::new(rng);

        // Pre-fill with None
        for _ in 0..num_octaves {
            octaves.push(None);
        }

        // Place anchor noise if in range
        if anchor_idx < num_octaves {
            octaves[anchor_idx] = Some(anchor_noise);
        }

        // Fill octaves backwards from anchor
        for i in (0..anchor_idx).rev() {
            if i < num_octaves {
                octaves[i] = Some(PerlinNoise::new(rng));
            } else {
                // Skip octave (consume RNG state)
                Self::skip_octave(rng);
            }
        }

        Self {
            octaves,
            first_octave,
            num_octaves,
        }
    }

    /// Skip an octave by consuming the expected RNG calls
    fn skip_octave(rng: &mut Xoroshiro128) {
        // Perlin noise consumes: 3 doubles + 256 ints for shuffle
        for _ in 0..262 {
            rng.next_int(256);
        }
    }

    /// Get the octave noise at index (from highest frequency).
    /// Index 0 is the highest frequency octave.
    fn get_octave(&self, index: usize) -> Option<&PerlinNoise> {
        let actual_idx = self.num_octaves.saturating_sub(1).saturating_sub(index);
        self.octaves.get(actual_idx).and_then(|o| o.as_ref())
    }

    /// Sample the octave noise with the legacy parameters.
    fn sample(&self, x: f64, y: f64, z: f64, y_scale: f64, y_offset: f64) -> f64 {
        let mut value = 0.0;
        let mut frequency = self.lowest_freq_input_factor();
        let mut amplitude = self.lowest_freq_value_factor();

        for i in 0..self.num_octaves {
            if let Some(octave) = &self.octaves[i] {
                let wrapped_x = Self::wrap(x * frequency);
                let wrapped_y = Self::wrap(y * frequency);
                let wrapped_z = Self::wrap(z * frequency);

                // Sample with y offset for smearing
                let noise_val =
                    octave.sample(wrapped_x, wrapped_y + y_offset * frequency, wrapped_z);
                value += amplitude * noise_val;
            }
            frequency *= 2.0;
            amplitude /= 2.0;
        }

        value
    }

    /// Wrap coordinate to prevent precision issues at large values.
    #[inline]
    fn wrap(d: f64) -> f64 {
        const ROUND_OFF: f64 = 33554432.0; // 2^25
        d - (d / ROUND_OFF + 0.5).floor() * ROUND_OFF
    }

    /// Get the lowest frequency input factor.
    fn lowest_freq_input_factor(&self) -> f64 {
        2.0_f64.powi(self.first_octave)
    }

    /// Get the lowest frequency value factor.
    fn lowest_freq_value_factor(&self) -> f64 {
        let n = self.num_octaves as i32;
        2.0_f64.powi(n - 1) / (2.0_f64.powi(n) - 1.0)
    }

    /// Calculate the maximum value for edge calculation.
    fn edge_value(&self, d: f64) -> f64 {
        let mut value = 0.0;
        let mut amplitude = self.lowest_freq_value_factor();

        for octave in &self.octaves {
            if octave.is_some() {
                value += amplitude * d;
            }
            amplitude /= 2.0;
        }

        value
    }
}

impl BlendedNoise {
    /// Base frequency constant (same as Java).
    const BASE_FREQUENCY: f64 = 684.412;

    /// Create a new BlendedNoise with the given parameters and seed.
    pub fn new(
        rng: &mut Xoroshiro128,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        // Create the three octave noises with legacy initialization
        // min_limit: octaves -15 to 0 (16 octaves)
        let min_limit_noise = LegacyOctaveNoise::new(rng, -15, 16);
        // max_limit: octaves -15 to 0 (16 octaves)
        let max_limit_noise = LegacyOctaveNoise::new(rng, -15, 16);
        // main: octaves -7 to 0 (8 octaves)
        let main_noise = LegacyOctaveNoise::new(rng, -7, 8);

        let xz_multiplier = Self::BASE_FREQUENCY * xz_scale;
        let y_multiplier = Self::BASE_FREQUENCY * y_scale;

        // Calculate max value
        let max_value = min_limit_noise.edge_value(y_multiplier + 2.0);

        Self {
            min_limit_noise,
            max_limit_noise,
            main_noise,
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
            xz_multiplier,
            y_multiplier,
            max_value,
        }
    }

    /// Create with default overworld settings.
    pub fn default_overworld(rng: &mut Xoroshiro128) -> Self {
        Self::new(rng, 1.0, 1.0, 80.0, 160.0, 8.0)
    }
}

impl DensityFunction for BlendedNoise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let block_x = ctx.block_x() as f64;
        let block_y = ctx.block_y() as f64;
        let block_z = ctx.block_z() as f64;

        // Scale coordinates
        let d = block_x * self.xz_multiplier;
        let e = block_y * self.y_multiplier;
        let f = block_z * self.xz_multiplier;

        // Main noise sampling coordinates (divided by factor)
        let g = d / self.xz_factor;
        let h = e / self.y_factor;
        let i = f / self.xz_factor;

        // Smear parameters
        let j = self.y_multiplier * self.smear_scale_multiplier;
        let k = j / self.y_factor;

        // Sample main noise (selector)
        let mut n = 0.0;
        let mut o = 1.0;

        for p in 0..8 {
            if let Some(_octave) = self.main_noise.get_octave(p) {
                let wrapped_g = LegacyOctaveNoise::wrap(g * o);
                let wrapped_h = LegacyOctaveNoise::wrap(h * o);
                let wrapped_i = LegacyOctaveNoise::wrap(i * o);
                // For main noise, we sample with smear
                n += self
                    .main_noise
                    .sample(wrapped_g, wrapped_h, wrapped_i, k * o, h * o)
                    / o;
            }
            o /= 2.0;
        }

        // Convert selector to 0-1 range
        let q = (n / 10.0 + 1.0) / 2.0;
        let is_max = q >= 1.0;
        let is_min = q <= 0.0;

        // Sample limit noises based on selector
        let mut l = 0.0; // min limit
        let mut m = 0.0; // max limit
        o = 1.0;

        for r in 0..16 {
            let s = LegacyOctaveNoise::wrap(d * o);
            let t = LegacyOctaveNoise::wrap(e * o);
            let u = LegacyOctaveNoise::wrap(f * o);
            let v = j * o;

            if !is_max {
                if let Some(_octave) = self.min_limit_noise.get_octave(r) {
                    l += self.min_limit_noise.sample(s, t, u, v, e * o) / o;
                }
            }

            if !is_min {
                if let Some(_octave) = self.max_limit_noise.get_octave(r) {
                    m += self.max_limit_noise.sample(s, t, u, v, e * o) / o;
                }
            }

            o /= 2.0;
        }

        // Blend and normalize
        let clamped_q = q.clamp(0.0, 1.0);
        let blended = lerp(clamped_q, l / 512.0, m / 512.0);
        blended / 128.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -self.max_value
    }

    fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// Old blended noise for legacy terrain compatibility (stub for backwards compat).
///
/// This is a simplified alias. Use BlendedNoise for the full implementation.
pub type OldBlendedNoise = BlendedNoise;

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

    #[test]
    fn test_blended_noise_deterministic() {
        // Test that BlendedNoise is deterministic with same seed
        let mut rng1 = Xoroshiro128::from_seed(12345);
        let mut rng2 = Xoroshiro128::from_seed(12345);

        let noise1 = BlendedNoise::default_overworld(&mut rng1);
        let noise2 = BlendedNoise::default_overworld(&mut rng2);

        let ctx = SinglePointContext::new(100, 64, 100);

        // Same seed should produce same results
        assert_eq!(noise1.compute(&ctx), noise2.compute(&ctx));
    }

    #[test]
    fn test_blended_noise_finite() {
        // Test that BlendedNoise produces finite values
        let mut rng = Xoroshiro128::from_seed(42);
        let noise = BlendedNoise::default_overworld(&mut rng);

        for x in [-100, 0, 100] {
            for y in [-64, 0, 64, 128] {
                for z in [-100, 0, 100] {
                    let ctx = SinglePointContext::new(x, y, z);
                    let value = noise.compute(&ctx);
                    assert!(
                        value.is_finite(),
                        "BlendedNoise produced non-finite value at ({}, {}, {}): {}",
                        x,
                        y,
                        z,
                        value
                    );
                }
            }
        }
    }
}
