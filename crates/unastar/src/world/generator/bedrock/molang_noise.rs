//! World-independent Molang `query.noise(x, z)` for the Bedrock generator.
//!
//! Authored BDS 1.26.51 controls match a fixed MT19937(2345) permutation,
//! three constructor offset draws and native float32 simplex arithmetic. The
//! permutation and final hash gradients are evaluated at compile time: no
//! world-seed mixing, RNG initialization, permutation copies or allocation at
//! runtime. Scalar/bit-pattern controls live in `bds_1_26_51_molang_noise.json`.

use super::rng::BedrockRandom;
use super::simplex::SimplexNoise;
use super::world::NoiseSource;

/// The same fixed biome-info field also participates in the measured native
/// frozen-ocean modifier. One immutable table serves both consumers.
pub(super) static BIOME_INFO_NOISE: SimplexNoise = {
    let mut random = BedrockRandom::new(2345);
    SimplexNoise::new(&mut random)
};

/// Stateless native noise intrinsic; deliberately takes no world seed.
pub struct MolangNoise;

impl NoiseSource for MolangNoise {
    #[inline]
    fn noise(&self, x: f32, z: f32) -> f32 {
        BIOME_INFO_NOISE.get_value_2d(x, z)
    }
}
