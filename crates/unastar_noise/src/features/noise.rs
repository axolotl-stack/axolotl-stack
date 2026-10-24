//! Noises used by decoration that are seeded from constants with the legacy
//! random, independent of the world seed:
//! - [`LegacyNoise`]: `NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(seed)), params)`
//!   (noise-based block state providers);
//! - [`biome_info_noise`]: `Biome.BIOME_INFO_NOISE`, a single-octave simplex noise
//!   seeded with 2345 (`noise_based_count`, `noise_threshold_count`).

use super::blocks::NoiseSpec;
use super::random::{LegacyRandom, java_string_hash};
use crate::noise::{PerlinNoise, SimplexNoise};
use std::sync::LazyLock;

/// A lazily built constant-seeded noise referenced from static provider data.
pub type NoiseRef = &'static LazyLock<LegacyNoise>;
/// `PerlinNoise.wrap`: keeps sample coordinates in a precise range.
#[inline]
fn wrap(v: f64) -> f64 {
    v - (v / 3.355_443_2e7 + 0.5).floor() * 3.355_443_2e7
}

/// One `PerlinNoise` built with the positional ("new factory") initialisation.
#[derive(Debug, Clone)]
struct Octaves {
    /// (noise, amplitude * value factor, input frequency)
    levels: Vec<(PerlinNoise, f64, f64)>,
}

impl Octaves {
    fn new(random: &mut LegacyRandom, spec: NoiseSpec) -> Self {
        // forkPositional on a legacy source: a factory seeded with nextLong();
        // fromHashOf(name) = new LegacyRandomSource(name.hashCode() ^ seed).
        let factory_seed = random.next_long();
        let n = spec.amplitudes.len() as i32;
        let lowest_input = 2f64.powi(spec.first_octave);
        let lowest_value = 2f64.powi(n - 1) / (2f64.powi(n) - 1.0);
        let mut levels = Vec::new();
        for (k, &amp) in spec.amplitudes.iter().enumerate() {
            if amp == 0.0 {
                continue;
            }
            let name = format!("octave_{}", spec.first_octave + k as i32);
            let mut r = LegacyRandom::new(java_string_hash(&name) as i64 ^ factory_seed);
            let noise = PerlinNoise::new(&mut r);
            let input = lowest_input * 2f64.powi(k as i32);
            let value = lowest_value / 2f64.powi(k as i32);
            levels.push((noise, amp * value, input));
        }
        Self { levels }
    }

    fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        for (noise, factor, input) in &self.levels {
            sum += factor * noise.sample(wrap(x * input), wrap(y * input), wrap(z * input));
        }
        sum
    }
}

/// `NormalNoise` seeded through a legacy `WorldgenRandom`.
#[derive(Debug, Clone)]
pub struct LegacyNoise {
    first: Octaves,
    second: Octaves,
    value_factor: f64,
}

impl LegacyNoise {
    /// `NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(seed)), spec)`.
    pub fn new(seed: i64, spec: NoiseSpec) -> Self {
        let mut random = LegacyRandom::new(seed);
        let first = Octaves::new(&mut random, spec);
        let second = Octaves::new(&mut random, spec);
        let nonzero: Vec<usize> = spec
            .amplitudes
            .iter()
            .enumerate()
            .filter(|(_, a)| **a != 0.0)
            .map(|(i, _)| i)
            .collect();
        let span = match (nonzero.first(), nonzero.last()) {
            (Some(lo), Some(hi)) => (hi - lo) as f64,
            _ => 0.0,
        };
        let expected_deviation = 0.1 * (1.0 + 1.0 / (span + 1.0));
        Self {
            first,
            second,
            value_factor: (1.0 / 6.0) / expected_deviation,
        }
    }

    /// `getValue(x, y, z)`.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        const F: f64 = 1.018_126_888_217_522_7;
        (self.first.sample(x, y, z) + self.second.sample(x * F, y * F, z * F)) * self.value_factor
    }
}

static BIOME_INFO_NOISE: LazyLock<SimplexNoise> =
    LazyLock::new(|| SimplexNoise::new(&mut LegacyRandom::new(2345)));

/// `Biome.BIOME_INFO_NOISE.getValue(x, z, false)`.
#[inline]
pub fn biome_info_noise(x: f64, z: f64) -> f64 {
    BIOME_INFO_NOISE.get_value_2d(x, z)
}
