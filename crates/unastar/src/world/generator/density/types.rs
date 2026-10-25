//! Core density function types for terrain generation.
//!
//! The NoiseRegistry is the only type kept here - all other types
//! are now provided by the unastar_noise crate.

use crate::world::generator::noise::{BlendedNoise, DoublePerlinNoise};
use crate::world::generator::xoroshiro::PositionalRandomFactory;
use std::simd::prelude::*;

use unastar_noise::{NOISE_PARAMS, NoiseRef, NoiseSource, overworld_settings, random_hashes};

/// Registry of instantiated noise functions (created from seed).
///
/// Uses a fixed-size Vec indexed by NoiseRef discriminant for O(1) lookup
/// instead of HashMap, which is critical for performance since noise lookups
/// are on the hot path of density function evaluation.
pub struct NoiseRegistry {
    noises: Vec<DoublePerlinNoise>,
    /// BlendedNoise for base_3d_noise (OldBlendedNoise)
    blended_noise: BlendedNoise,
    seed: i64,
}

impl NoiseRegistry {
    /// Instantiate every noise for a seed the way Java's `RandomState` does: each noise is
    /// seeded from the world's positional factory by `fromHashOf("minecraft:<noise>")`,
    /// the blended terrain noise by `fromHashOf("minecraft:terrain")`.
    pub fn new(seed: i64) -> Self {
        let base = PositionalRandomFactory::new(seed);
        let mut noises = Vec::with_capacity(NoiseRef::COUNT);
        for (noise_ref, params) in NOISE_PARAMS.iter() {
            debug_assert_eq!(
                noises.len(),
                *noise_ref as usize,
                "NOISE_PARAMS out of order"
            );
            let mut rng = base.from_hash(params.hash);
            noises.push(DoublePerlinNoise::new(
                &mut rng,
                params.amplitudes,
                params.first_octave,
            ));
        }

        let params = overworld_settings::BLENDED_NOISE;
        let mut blended_rng = base.from_hash(random_hashes::TERRAIN);
        let blended_noise = BlendedNoise::new(
            &mut blended_rng,
            params.xz_scale,
            params.y_scale,
            params.xz_factor,
            params.y_factor,
            params.smear_scale_multiplier,
        );

        Self {
            noises,
            blended_noise,
            seed,
        }
    }

    /// Get a noise by reference - O(1) array index.
    #[inline]
    pub fn get(&self, noise_ref: NoiseRef) -> &DoublePerlinNoise {
        // SAFETY: NoiseRef is repr(usize) and we initialized all variants
        unsafe { self.noises.get_unchecked(noise_ref as usize) }
    }

    /// Get the seed used to create this registry.
    pub fn seed(&self) -> i64 {
        self.seed
    }
}

/// Implement the NoiseSource trait for NoiseRegistry so it can be used
/// with the AOT-compiled density functions from unastar_noise.
impl NoiseSource for NoiseRegistry {
    #[inline]
    fn sample(&self, noise_ref: NoiseRef, x: f64, y: f64, z: f64) -> f64 {
        self.get(noise_ref).sample(x, y, z)
    }

    #[inline]
    fn sample_4(&self, noise_ref: NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        self.get(noise_ref).sample_4(x, y, z)
    }

    #[inline]
    fn sample_blended_noise(
        &self,
        x: f64,
        y: f64,
        z: f64,
        _xz_scale: f64,
        _y_scale: f64,
        _xz_factor: f64,
        _y_factor: f64,
        _smear_scale_multiplier: f64,
    ) -> f64 {
        // Note: The parameters are already baked into the BlendedNoise instance
        // This is a simplification - we could support dynamic parameters if needed
        self.blended_noise.sample(x, y, z)
    }

    #[inline]
    fn sample_blended_noise_4(
        &self,
        x: f64,
        y: f64x4,
        z: f64,
        _xz_scale: f64,
        _y_scale: f64,
        _xz_factor: f64,
        _y_factor: f64,
        _smear_scale_multiplier: f64,
    ) -> f64x4 {
        self.blended_noise.sample_4(x, y, z)
    }
}
