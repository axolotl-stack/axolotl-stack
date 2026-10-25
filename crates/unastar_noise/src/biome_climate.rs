//! Biome temperature at a block, as Java's surface rules (`minecraft:temperature`),
//! iceberg melting and snow/ice placement read it.
//!
//! The temperature is the biome's base temperature, replaced by 0.2 in patches of frozen
//! biomes, and lowered above `sea_level + 17` by a fixed-seed simplex noise. The three
//! noises are seeded with constants (Java's legacy random), not with the world seed.

use std::sync::LazyLock;

use crate::java_biomes::JavaBiome;
use crate::noise::SimplexNoise;
use crate::xoroshiro::JavaRandom;

/// Java's legacy multi-octave simplex noise (`PerlinSimplexNoise`), octaves <= 0.
struct LegacySimplexNoise {
    /// Index 0 is the highest-frequency octave (octave 0).
    levels: Vec<Option<SimplexNoise>>,
    highest_freq_input_factor: f64,
    highest_freq_value_factor: f64,
}

impl LegacySimplexNoise {
    /// `octaves` ascending, all <= 0 and containing 0.
    fn new(seed: i64, octaves: &[i32]) -> Self {
        let first = octaves[0];
        let last = *octaves.last().expect("at least one octave");
        assert!(
            last == 0 && octaves.contains(&0),
            "only octave sets ending at 0 are used"
        );
        let count = (-first + last + 1) as usize;
        let mut random = JavaRandom::from_seed(seed);
        let first_noise = SimplexNoise::from_legacy(&mut random);
        let mut levels: Vec<Option<SimplexNoise>> = (0..count).map(|_| None).collect();
        levels[0] = Some(first_noise);
        for (m, level) in levels.iter_mut().enumerate().skip(1) {
            if octaves.contains(&-(m as i32)) {
                *level = Some(SimplexNoise::from_legacy(&mut random));
            } else {
                random.skip(262);
            }
        }
        Self {
            levels,
            highest_freq_input_factor: 1.0,
            highest_freq_value_factor: 1.0 / ((1u64 << count) as f64 - 1.0),
        }
    }

    fn get_value(&self, x: f64, y: f64) -> f64 {
        let mut value = 0.0;
        let mut input = self.highest_freq_input_factor;
        let mut weight = self.highest_freq_value_factor;
        for noise in self.levels.iter().flatten() {
            value += noise.get_value_2d(x * input, y * input) * weight;
            input /= 2.0;
            weight *= 2.0;
        }
        value
    }
}

static TEMPERATURE_NOISE: LazyLock<LegacySimplexNoise> =
    LazyLock::new(|| LegacySimplexNoise::new(1234, &[0]));
static FROZEN_TEMPERATURE_NOISE: LazyLock<LegacySimplexNoise> =
    LazyLock::new(|| LegacySimplexNoise::new(3456, &[-2, -1, 0]));
static BIOME_INFO_NOISE: LazyLock<LegacySimplexNoise> =
    LazyLock::new(|| LegacySimplexNoise::new(2345, &[0]));

/// Base temperature after the `frozen` modifier.
fn modified_temperature(biome: JavaBiome, x: i32, z: i32) -> f32 {
    let data = biome.data();
    if data.frozen {
        let d = FROZEN_TEMPERATURE_NOISE.get_value(x as f64 * 0.05, z as f64 * 0.05) * 7.0;
        let e = BIOME_INFO_NOISE.get_value(x as f64 * 0.2, z as f64 * 0.2);
        if d + e < 0.3 && BIOME_INFO_NOISE.get_value(x as f64 * 0.09, z as f64 * 0.09) < 0.8 {
            return 0.2;
        }
    }
    data.temperature
}

/// Temperature of `biome` at a block.
pub fn temperature(biome: JavaBiome, x: i32, y: i32, z: i32, sea_level: i32) -> f32 {
    let base = modified_temperature(biome, x, z);
    let threshold = sea_level + 17;
    if y > threshold {
        let noise = (TEMPERATURE_NOISE.get_value((x as f32 / 8.0) as f64, (z as f32 / 8.0) as f64)
            * 8.0) as f32;
        return base - (noise + y as f32 - threshold as f32) * 0.05 / 40.0;
    }
    base
}

/// Snow instead of rain (temperature below 0.15).
pub fn cold_enough_to_snow(biome: JavaBiome, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
    temperature(biome, x, y, z, sea_level) < 0.15
}

/// Frozen-ocean icebergs are lowered by 2 where the temperature exceeds 0.1.
pub fn should_melt_frozen_ocean_iceberg_slightly(
    biome: JavaBiome,
    x: i32,
    y: i32,
    z: i32,
    sea_level: i32,
) -> bool {
    temperature(biome, x, y, z, sea_level) > 0.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_frozen_biome_below_threshold_keeps_base_temperature() {
        let desert = JavaBiome::from_name("minecraft:desert").unwrap();
        assert_eq!(
            temperature(desert, 10, 63, 10, 63),
            desert.data().temperature
        );
        assert!(!cold_enough_to_snow(desert, 10, 63, 10, 63));
    }

    #[test]
    fn height_cools_above_sea_level_plus_17() {
        let plains = JavaBiome::from_name("minecraft:plains").unwrap();
        let low = temperature(plains, 0, 80, 0, 63);
        let high = temperature(plains, 0, 250, 0, 63);
        assert!(high < low);
    }
}
