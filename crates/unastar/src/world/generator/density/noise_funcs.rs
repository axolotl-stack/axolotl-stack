//! Noise-based density functions.
//!
//! These functions sample noise values to create terrain variation.
//! They connect to the noise infrastructure in noise.rs.

use super::context::FunctionContext;
use super::function::{DensityFunction, Visitor};
use crate::world::generator::noise::DoublePerlinNoise;
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::sync::Arc;

/// Parameters for noise generation.
#[derive(Debug, Clone)]
pub struct NoiseParams {
    /// First octave (negative = lower frequency, larger features).
    pub first_octave: i32,
    /// Amplitudes for each octave.
    pub amplitudes: Vec<f64>,
}

impl NoiseParams {
    /// Create new noise parameters.
    pub fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        Self {
            first_octave,
            amplitudes,
        }
    }

    /// Get the maximum possible value this noise can produce.
    pub fn max_value(&self) -> f64 {
        // Sum of all positive amplitudes
        self.amplitudes.iter().filter(|&&a| a > 0.0).sum::<f64>() * 2.0
    }
}

/// Holder for noise data with optional instantiated noise.
#[derive(Clone)]
pub struct NoiseHolder {
    /// Noise parameters.
    pub params: NoiseParams,
    /// Instantiated noise (populated during wiring).
    pub noise: Option<Arc<DoublePerlinNoise>>,
}

impl NoiseHolder {
    /// Create a new noise holder with parameters.
    pub fn new(params: NoiseParams) -> Self {
        Self {
            params,
            noise: None,
        }
    }

    /// Create a noise holder with instantiated noise.
    pub fn with_noise(params: NoiseParams, noise: DoublePerlinNoise) -> Self {
        Self {
            params,
            noise: Some(Arc::new(noise)),
        }
    }

    /// Instantiate the noise from a seed.
    pub fn instantiate(&mut self, seed: i64) {
        let mut rng = Xoroshiro128::from_seed(seed);
        let noise = DoublePerlinNoise::new(
            &mut rng,
            &self.params.amplitudes,
            self.params.first_octave,
        );
        self.noise = Some(Arc::new(noise));
    }

    /// Get noise value at position.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise.as_ref().map_or(0.0, |n| n.sample(x, y, z))
    }

    /// Get maximum value this noise can produce.
    pub fn max_value(&self) -> f64 {
        self.params.max_value()
    }
}

/// Basic noise sampling density function.
#[derive(Clone)]
pub struct Noise {
    /// Noise holder with parameters and instantiated noise.
    pub noise: NoiseHolder,
    /// XZ coordinate scale.
    pub xz_scale: f64,
    /// Y coordinate scale.
    pub y_scale: f64,
}

impl Noise {
    /// Create a new noise density function.
    pub fn new(noise: NoiseHolder, xz_scale: f64, y_scale: f64) -> Self {
        Self {
            noise,
            xz_scale,
            y_scale,
        }
    }

    /// Create with default scales (1.0).
    pub fn with_holder(noise: NoiseHolder) -> Self {
        Self::new(noise, 1.0, 1.0)
    }

    pub fn with_scales(self, xz_scale: f64, y_scale: f64) -> Self {
        Self {
            noise: self.noise,
            xz_scale,
            y_scale,
        }
    }
}

impl DensityFunction for Noise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let x = ctx.block_x() as f64 * self.xz_scale;
        let y = ctx.block_y() as f64 * self.y_scale;
        let z = ctx.block_z() as f64 * self.xz_scale;
        self.noise.get_value(x, y, z)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -self.noise.max_value()
    }

    fn max_value(&self) -> f64 {
        self.noise.max_value()
    }
}

/// Shifted noise density function (domain warping).
///
/// Samples noise at coordinates offset by shift functions.
#[derive(Clone)]
pub struct ShiftedNoise {
    /// X shift function.
    pub shift_x: Arc<dyn DensityFunction>,
    /// Y shift function.
    pub shift_y: Arc<dyn DensityFunction>,
    /// Z shift function.
    pub shift_z: Arc<dyn DensityFunction>,
    /// Noise holder.
    pub noise: NoiseHolder,
    /// XZ coordinate scale.
    pub xz_scale: f64,
    /// Y coordinate scale.
    pub y_scale: f64,
}

impl ShiftedNoise {
    /// Create a new shifted noise function.
    pub fn new(
        shift_x: Arc<dyn DensityFunction>,
        shift_y: Arc<dyn DensityFunction>,
        shift_z: Arc<dyn DensityFunction>,
        noise: NoiseHolder,
        xz_scale: f64,
        y_scale: f64,
    ) -> Self {
        Self {
            shift_x,
            shift_y,
            shift_z,
            noise,
            xz_scale,
            y_scale,
        }
    }
}

impl DensityFunction for ShiftedNoise {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let sx = self.shift_x.compute(ctx);
        let sy = self.shift_y.compute(ctx);
        let sz = self.shift_z.compute(ctx);

        let x = (ctx.block_x() as f64 + sx) * self.xz_scale;
        let y = (ctx.block_y() as f64 + sy) * self.y_scale;
        let z = (ctx.block_z() as f64 + sz) * self.xz_scale;

        self.noise.get_value(x, y, z)
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_shift_x = self.shift_x.map_all(visitor);
        let new_shift_y = self.shift_y.map_all(visitor);
        let new_shift_z = self.shift_z.map_all(visitor);
        visitor.apply(Arc::new(ShiftedNoise::new(
            new_shift_x,
            new_shift_y,
            new_shift_z,
            self.noise.clone(),
            self.xz_scale,
            self.y_scale,
        )))
    }

    fn min_value(&self) -> f64 {
        -self.noise.max_value()
    }

    fn max_value(&self) -> f64 {
        self.noise.max_value()
    }
}

/// ShiftA density function.
///
/// Generates X or Z shift values for domain warping.
/// Samples at (x * 0.25, 0, z * 0.25) and multiplies by 4.
#[derive(Clone)]
pub struct ShiftA {
    /// Noise holder.
    pub noise: NoiseHolder,
}

impl ShiftA {
    /// Create a new ShiftA function.
    pub fn new(noise: NoiseHolder) -> Self {
        Self { noise }
    }
}

impl DensityFunction for ShiftA {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        self.noise.get_value(
            ctx.block_x() as f64 * 0.25,
            0.0,
            ctx.block_z() as f64 * 0.25,
        ) * 4.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -self.noise.max_value() * 4.0
    }

    fn max_value(&self) -> f64 {
        self.noise.max_value() * 4.0
    }
}

/// ShiftB density function.
///
/// Similar to ShiftA but samples at different offset.
/// Samples at (z * 0.25, x * 0.25, 0) and multiplies by 4.
#[derive(Clone)]
pub struct ShiftB {
    /// Noise holder.
    pub noise: NoiseHolder,
}

impl ShiftB {
    /// Create a new ShiftB function.
    pub fn new(noise: NoiseHolder) -> Self {
        Self { noise }
    }
}

impl DensityFunction for ShiftB {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        self.noise.get_value(
            ctx.block_z() as f64 * 0.25,
            ctx.block_x() as f64 * 0.25,
            0.0,
        ) * 4.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -self.noise.max_value() * 4.0
    }

    fn max_value(&self) -> f64 {
        self.noise.max_value() * 4.0
    }
}

/// Generic Shift density function.
///
/// Samples at (x * 0.25, y * 0.25, z * 0.25) and multiplies by 4.
#[derive(Clone)]
pub struct Shift {
    /// Noise holder.
    pub noise: NoiseHolder,
}

impl Shift {
    /// Create a new Shift function.
    pub fn new(noise: NoiseHolder) -> Self {
        Self { noise }
    }
}

impl DensityFunction for Shift {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        self.noise.get_value(
            ctx.block_x() as f64 * 0.25,
            ctx.block_y() as f64 * 0.25,
            ctx.block_z() as f64 * 0.25,
        ) * 4.0
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        visitor.apply(Arc::new(self.clone()))
    }

    fn min_value(&self) -> f64 {
        -self.noise.max_value() * 4.0
    }

    fn max_value(&self) -> f64 {
        self.noise.max_value() * 4.0
    }
}

/// Weird scaled sampler for terrain variation.
///
/// Samples noise at coordinates scaled by input-dependent rarity.
#[derive(Clone)]
pub struct WeirdScaledSampler {
    /// Input function that determines rarity/scale.
    pub input: Arc<dyn DensityFunction>,
    /// Noise holder.
    pub noise: NoiseHolder,
    /// Rarity type.
    pub rarity_type: RarityType,
}

/// Rarity calculation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RarityType {
    /// Type 1: Used for some terrain features.
    Type1,
    /// Type 2: Used for other terrain features.
    Type2,
}

impl WeirdScaledSampler {
    /// Create a new weird scaled sampler.
    pub fn new(
        input: Arc<dyn DensityFunction>,
        noise: NoiseHolder,
        rarity_type: RarityType,
    ) -> Self {
        Self {
            input,
            noise,
            rarity_type,
        }
    }

    /// Calculate rarity value from input.
    fn get_rarity_value(&self, input: f64) -> f64 {
        match self.rarity_type {
            RarityType::Type1 => {
                if input < -0.75 {
                    0.5
                } else if input < -0.5 {
                    0.75
                } else if input < 0.5 {
                    1.0
                } else if input < 0.75 {
                    2.0
                } else {
                    3.0
                }
            }
            RarityType::Type2 => {
                if input < -0.5 {
                    0.75
                } else if input < 0.0 {
                    1.0
                } else if input < 0.5 {
                    1.5
                } else {
                    2.0
                }
            }
        }
    }
}

impl DensityFunction for WeirdScaledSampler {
    fn compute(&self, ctx: &dyn FunctionContext) -> f64 {
        let rarity = self.get_rarity_value(self.input.compute(ctx));
        self.noise
            .get_value(
                ctx.block_x() as f64 / rarity,
                ctx.block_y() as f64 / rarity,
                ctx.block_z() as f64 / rarity,
            )
            .abs()
            * rarity
    }

    fn map_all(&self, visitor: &dyn Visitor) -> Arc<dyn DensityFunction> {
        let new_input = self.input.map_all(visitor);
        visitor.apply(Arc::new(WeirdScaledSampler::new(
            new_input,
            self.noise.clone(),
            self.rarity_type,
        )))
    }

    fn min_value(&self) -> f64 {
        0.0
    }

    fn max_value(&self) -> f64 {
        // Maximum rarity (3.0) * max noise value
        self.noise.max_value() * 3.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::context::SinglePointContext;
    use crate::world::generator::density::math::Constant;

    #[test]
    fn test_noise_params() {
        let params = NoiseParams::new(-7, vec![1.0, 1.0, 1.0, 1.0]);
        assert_eq!(params.first_octave, -7);
        assert!(params.max_value() > 0.0);
    }

    #[test]
    fn test_noise_holder_no_instantiation() {
        let params = NoiseParams::new(-7, vec![1.0, 1.0]);
        let holder = NoiseHolder::new(params);
        // Without instantiation, should return 0
        assert_eq!(holder.get_value(0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn test_shift_a() {
        let params = NoiseParams::new(-3, vec![1.0, 1.0]);
        let mut holder = NoiseHolder::new(params);
        holder.instantiate(12345);
        let shift = ShiftA::new(holder);

        let ctx = SinglePointContext::new(100, 64, 200);
        let value = shift.compute(&ctx);
        // Should be within bounds
        assert!(value >= shift.min_value() && value <= shift.max_value());
    }

    #[test]
    fn test_weird_scaled_rarity() {
        let params = NoiseParams::new(-3, vec![1.0]);
        let holder = NoiseHolder::new(params);
        let input = Arc::new(Constant::new(0.8));
        let sampler = WeirdScaledSampler::new(input, holder, RarityType::Type1);

        // At input 0.8, rarity should be 3.0
        assert_eq!(sampler.get_rarity_value(0.8), 3.0);
        assert_eq!(sampler.get_rarity_value(-0.8), 0.5);
        assert_eq!(sampler.get_rarity_value(0.0), 1.0);
    }
}
