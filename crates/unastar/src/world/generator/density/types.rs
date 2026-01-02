//! Core density function types for terrain generation.
//!
//! The NoiseRegistry is the only type kept here - all other types
//! are now provided by the unastar_noise crate.

use crate::world::generator::noise::{DoublePerlinNoise, BlendedNoise};
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::simd::prelude::*;

use unastar_noise::{NoiseRef, NoiseSource, NOISE_PARAMS};

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
    /// Create a new noise registry with all noises instantiated from the seed.
    pub fn new(seed: i64) -> Self {
        // Pre-allocate Vec with exact size
        let mut noises = Vec::with_capacity(NoiseRef::COUNT);

        // Initialize in order so index matches enum discriminant
        for (noise_ref, params) in NOISE_PARAMS.iter() {
            debug_assert_eq!(noises.len(), *noise_ref as usize, "NOISE_PARAMS out of order");
            let salted_seed = Self::hash_seed(seed, *noise_ref);
            let mut rng = Xoroshiro128::from_seed(salted_seed);
            let noise = DoublePerlinNoise::new(&mut rng, &params.amplitudes, params.first_octave);
            noises.push(noise);
        }

        // Create BlendedNoise for base_3d_noise
        // Use a specific seed salt for BlendedNoise
        let blended_seed = seed.wrapping_mul(31).wrapping_add(1000);
        let mut blended_rng = Xoroshiro128::from_seed(blended_seed);
        // Default parameters for overworld base_3d_noise (from worldgen JSON):
        // xz_scale=0.25, y_scale=0.125, xz_factor=80.0, y_factor=160.0, smear_scale_multiplier=8.0
        let blended_noise = BlendedNoise::new(
            &mut blended_rng,
            0.25,   // xz_scale
            0.125,  // y_scale
            80.0,   // xz_factor
            160.0,  // y_factor
            8.0,    // smear_scale_multiplier
        );

        Self { noises, blended_noise, seed }
    }

    /// Get a noise by reference - O(1) array index.
    #[inline]
    pub fn get(&self, noise_ref: NoiseRef) -> &DoublePerlinNoise {
        // SAFETY: NoiseRef is repr(usize) and we initialized all variants
        unsafe { self.noises.get_unchecked(noise_ref as usize) }
    }

    /// Hash a seed with a noise ref to create unique noise instances.
    fn hash_seed(seed: i64, noise_ref: NoiseRef) -> i64 {
        let salt = noise_ref as u64;
        seed.wrapping_mul(31).wrapping_add(salt as i64)
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
    fn sample_blended_noise(&self, x: f64, y: f64, z: f64, _xz_scale: f64, _y_scale: f64, _xz_factor: f64, _y_factor: f64, _smear_scale_multiplier: f64) -> f64 {
        // Note: The parameters are already baked into the BlendedNoise instance
        // This is a simplification - we could support dynamic parameters if needed
        self.blended_noise.sample(x, y, z)
    }

    #[inline]
    fn sample_blended_noise_4(&self, x: f64, y: f64x4, z: f64, _xz_scale: f64, _y_scale: f64, _xz_factor: f64, _y_factor: f64, _smear_scale_multiplier: f64) -> f64x4 {
        self.blended_noise.sample_4(x, y, z)
    }
}
