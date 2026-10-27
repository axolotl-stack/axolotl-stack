//! Native weighted noise keeps amplitude and persistence separate without
//! enlarging the calibrated Perlin records or adding a second allocation.

use super::{
    DoublePerlinNoise, OctaveNoise, PerlinNoise, seeded_octaves, wrap_coord_java,
    wrap_coord_java_simd,
};
use crate::features::random::LegacyRandom;
use crate::xoroshiro::Xoroshiro128;
use std::simd::f64x4;

/// A native octave retains its value factor separately from the amplitude.
#[derive(Debug, Clone, Default)]
pub struct NativeOctave {
    perlin: PerlinNoise,
    value_factor: f64,
}

/// Native scalar/SIMD octave storage; calibrated storage remains unchanged.
pub type NativeOctaveNoise = OctaveNoise<NativeOctave>;

/// Native NormalNoise with statically selected weighted octave storage.
pub type NativeDoublePerlinNoise = DoublePerlinNoise<NativeOctave>;

impl NativeOctaveNoise {
    /// Modern positional octave streams with exact native persistence.
    pub fn new_java(rng: &mut Xoroshiro128, amplitudes: &[f64], omin: i32) -> Self {
        let len = amplitudes.len() as i32;
        let persistence = 2.0_f64.powi(len - 1) / (2.0_f64.powi(len) - 1.0);
        Self {
            octaves: seeded_octaves(
                rng,
                amplitudes,
                omin,
                persistence,
                |mut perlin, amp, value_factor| {
                    perlin.amplitude = amp;
                    NativeOctave {
                        perlin,
                        value_factor,
                    }
                },
            ),
        }
    }
    /// Modern octave splitting over a legacy positional factory. The factory takes
    /// one signed Java long, then XORs each generated `octave_<n>` string hash.
    pub fn new_legacy(rng: &mut LegacyRandom, amplitudes: &[f64], omin: i32) -> Self {
        let factory_seed = rng.next_long();
        let count = amplitudes.iter().filter(|&&amp| amp != 0.0).count();
        let mut octaves = Vec::with_capacity(count);
        let len = amplitudes.len() as i32;
        let mut persistence = 2.0_f64.powi(len - 1) / (2.0_f64.powi(len) - 1.0);
        let mut lacunarity = 1.0;
        for (index, &amp) in amplitudes.iter().enumerate() {
            if amp != 0.0 {
                let hash = crate::random_hashes::legacy_octave(omin + index as i32);
                let mut octave_rng = LegacyRandom::new(factory_seed ^ i64::from(hash));
                let mut noise = PerlinNoise::new(&mut octave_rng);
                noise.amplitude = amp;
                noise.lacunarity = lacunarity;
                octaves.push(NativeOctave {
                    perlin: noise,
                    value_factor: persistence,
                });
            }
            persistence /= 2.0;
            lacunarity *= 2.0;
        }
        Self { octaves }
    }

    /// The fixed -7/[1,1] legacy Nether biome stream used by RandomState's
    /// temperature and vegetation overrides. Draw order is descending octaves.
    fn legacy_nether_biome(rng: &mut LegacyRandom) -> Self {
        let _zero_octave = PerlinNoise::new(rng);
        rng.consume_count(5 * 262);
        let mut upper = PerlinNoise::new(rng);
        let lower = PerlinNoise::new(rng);
        upper.lacunarity = 2.0;
        Self {
            octaves: vec![
                NativeOctave {
                    perlin: lower,
                    value_factor: 2.0 / 3.0,
                },
                NativeOctave {
                    perlin: upper,
                    value_factor: 1.0 / 3.0,
                },
            ],
        }
    }
    /// Java's scalar octave summation and per-octave coordinate wrapping.
    #[inline]
    pub fn sample_java(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut value = 0.0;
        for octave in &self.octaves {
            let lf = octave.perlin.lacunarity;
            let sample = octave.perlin.sample_java(
                wrap_coord_java(x * lf),
                wrap_coord_java(y * lf),
                wrap_coord_java(z * lf),
            );
            value += octave.perlin.amplitude * sample * octave.value_factor;
        }
        value
    }
    #[inline]
    pub fn sample_java_4(&self, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        let mut values = f64x4::splat(0.0);
        for octave in &self.octaves {
            let lf = f64x4::splat(octave.perlin.lacunarity);
            let sample = octave.perlin.sample_java_4(
                wrap_coord_java_simd(x * lf),
                wrap_coord_java_simd(y * lf),
                wrap_coord_java_simd(z * lf),
            );
            values +=
                f64x4::splat(octave.perlin.amplitude) * sample * f64x4::splat(octave.value_factor);
        }
        values
    }
}

impl NativeDoublePerlinNoise {
    /// Xoroshiro NormalNoise with native Java normalization rather than the
    /// calibrated table used by the existing fused terrain backend.
    pub fn new_java(rng: &mut Xoroshiro128, amplitudes: &[f64], omin: i32) -> Self {
        Self {
            oct_a: NativeOctaveNoise::new_java(rng, amplitudes, omin),
            oct_b: NativeOctaveNoise::new_java(rng, amplitudes, omin),
            amplitude: Self::native_amplitude(amplitudes),
            frequency: 2.0_f64.powi(omin),
        }
    }

    /// NormalNoise constructed from a legacy positional random source.
    pub fn new_legacy(rng: &mut LegacyRandom, amplitudes: &[f64], omin: i32) -> Self {
        Self {
            oct_a: NativeOctaveNoise::new_legacy(rng, amplitudes, omin),
            oct_b: NativeOctaveNoise::new_legacy(rng, amplitudes, omin),
            amplitude: Self::native_amplitude(amplitudes),
            frequency: 2.0_f64.powi(omin),
        }
    }

    /// Native fixed legacy Nether climate override; not ordinary hashed NormalNoise.
    pub fn legacy_nether_biome(rng: &mut LegacyRandom) -> Self {
        Self {
            oct_a: NativeOctaveNoise::legacy_nether_biome(rng),
            oct_b: NativeOctaveNoise::legacy_nether_biome(rng),
            amplitude: Self::native_amplitude(&[1.0, 1.0]),
            frequency: 2.0_f64.powi(-7),
        }
    }

    fn native_amplitude(amplitudes: &[f64]) -> f64 {
        let first = amplitudes
            .iter()
            .position(|&amp| amp != 0.0)
            .map_or(i32::MAX, |index| index as i32);
        let last = amplitudes
            .iter()
            .rposition(|&amp| amp != 0.0)
            .map_or(i32::MIN, |index| index as i32);
        let span = last.wrapping_sub(first);
        (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / (f64::from(span) + 1.0)))
    }
    /// Native Java scalar normal-noise sampling, explicitly separate from fused math.
    #[inline]
    pub fn sample_java(&self, x: f64, y: f64, z: f64) -> f64 {
        const F: f64 = 337.0 / 331.0;
        let nx = x * self.frequency;
        let ny = y * self.frequency;
        let nz = z * self.frequency;
        (self.oct_a.sample_java(nx, ny, nz) + self.oct_b.sample_java(nx * F, ny * F, nz * F))
            * self.amplitude
    }
    #[inline]
    pub fn sample_java_4(&self, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        let frequency = f64x4::splat(self.frequency);
        let factor = f64x4::splat(337.0 / 331.0);
        let x = x * frequency;
        let y = y * frequency;
        let z = z * frequency;
        (self.oct_a.sample_java_4(x, y, z)
            + self.oct_b.sample_java_4(x * factor, y * factor, z * factor))
            * f64x4::splat(self.amplitude)
    }
}
