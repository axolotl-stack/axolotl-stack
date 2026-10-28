//! Surface-builder noises: measured fixed frozen climate and world-seeded models.
//!
//! Frozen-ocean temperature uses fixed native MT19937 simplex fields, matching
//! every water/ice classification in the BDS fixture. Other fields remain
//! world/name-seeded "normal noise" models ([`DoublePerlinNoise`]); their exact
//! BDS seeding and frequencies remain unmeasured.
//!
//! The current model uses `noise * 2.75 + 3` for material depth and
//! `noise * 8.25` for legacy builder thresholds. The correspondence between
//! these scales and the shipped adjustment ranges is a hypothesis, not a
//! BDS measurement. Badlands, iceberg and swamp noise frequencies are also
//! unmeasured: to confirm by BDS fixture.

use super::super::molang_noise::BIOME_INFO_NOISE;
use super::super::rng::BedrockRandom;
use super::super::simplex::SimplexNoise;
use super::{BiomeData, Builder, NoiseGradient};
use unastar_noise::{DoublePerlinNoise, Xoroshiro128};

/// Lowest octave `unastar_noise` can seed (it carries the octave hashes of
/// octaves `-12..=0`).
const MIN_OCTAVE: i32 = -12;
/// Highest octave `unastar_noise` can seed.
const MAX_OCTAVE: i32 = 0;

// Native counterpart of the frozen modifier's octaves [-2, -1, 0], in
// highest-frequency-first order. One continued MT stream seeds all octaves.
static FROZEN_TEMPERATURE: [SimplexNoise; 3] = {
    let mut random = BedrockRandom::new(3456);
    [
        SimplexNoise::new(&mut random),
        SimplexNoise::new(&mut random),
        SimplexNoise::new(&mut random),
    ]
};

/// Number of entries of the badlands band sequence (it repeats every 64 y).
pub(super) const BAND_COUNT: usize = 64;

/// One entry of the badlands band sequence. `Base` and `White` are the
/// biome's `hard_clay_material` and `clay_material`; the others are fixed
/// terracotta colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Band {
    Base,
    Orange,
    Yellow,
    Brown,
    Red,
    White,
    LightGray,
}

/// A normal noise with a coordinate pre-scale (see [`Normal::new`]).
#[derive(Debug, Clone)]
struct Normal {
    noise: DoublePerlinNoise,
    coord_scale: f64,
}

impl Normal {
    /// Normal noise with octaves `first_octave..first_octave + len`.
    ///
    /// `unastar_noise` only seeds octaves `-12..=0`; a descriptor outside that
    /// window is shifted into it and the coordinates are scaled by
    /// `2^(first_octave - shifted)`, which gives the same frequencies (only
    /// the per-octave seeds differ, which parity does not depend on).
    fn new(seed: i64, name: &str, first_octave: i32, amplitudes: &[f64]) -> Self {
        let len = amplitudes.len() as i32;
        assert!(
            len <= MAX_OCTAVE - MIN_OCTAVE + 1,
            "noise `{name}` has {len} octaves; at most {} are supported",
            MAX_OCTAVE - MIN_OCTAVE + 1
        );
        let omin = first_octave.min(MAX_OCTAVE + 1 - len).max(MIN_OCTAVE);
        let mut rng = named_rng(seed, name);
        Self {
            noise: DoublePerlinNoise::new(&mut rng, amplitudes, omin),
            coord_scale: 2f64.powi(first_octave - omin),
        }
    }

    #[inline]
    fn at(&self, x: f64, y: f64, z: f64) -> f64 {
        let s = self.coord_scale;
        self.noise.sample(x * s, y * s, z * s)
    }

    #[inline]
    fn at_2d(&self, x: f64, z: f64) -> f64 {
        self.at(x, 0.0, z)
    }
}

/// The noise of one `minecraft:noise_gradient` descriptor.
#[derive(Debug, Clone)]
struct GradientNoise {
    name: &'static str,
    first_octave: i32,
    amplitudes: &'static [f32],
    noise: Normal,
}

impl GradientNoise {
    fn matches(&self, g: &NoiseGradient) -> bool {
        self.name == g.noise_name
            && self.first_octave == g.first_octave
            && self.amplitudes == g.amplitudes
    }
}

/// Every noise the surface builders sample, built once per world.
#[derive(Debug, Clone)]
pub struct SurfaceNoises {
    /// Layer depth and material adjustments (vanilla `surface` noise:
    /// octaves 1/64..1/16 at the default 1/16 coordinate scale).
    surface: Normal,
    /// Vertical offset of the badlands bands (sampled at x/512).
    band_offset: Normal,
    /// Badlands band colours, indexed by `y mod 64`.
    bands: [Band; BAND_COUNT],
    /// Eroded badlands pillar shape and roof.
    pillar: Normal,
    pillar_roof: Normal,
    /// Frozen ocean iceberg shape and roof.
    iceberg: Normal,
    iceberg_roof: Normal,
    /// Unmeasured world-seeded patch model for swamp puddles/nether beaches.
    /// Frozen ocean instead uses the measured fixed biome-info field above.
    biome_info: Normal,
    /// Material choice of the capped (nether) builder's floors / ceilings.
    capped_floor: Normal,
    capped_ceiling: Normal,
    /// One noise per distinct `noise_gradient` descriptor of the biome table.
    gradients: Vec<GradientNoise>,
}

impl SurfaceNoises {
    /// Noises for `seed`, including every `noise_gradient` descriptor of the
    /// compiled biome table (`generated::BIOMES`).
    pub fn new(seed: i64) -> Self {
        Self::with_biomes(seed, super::super::generated::BIOMES)
    }

    /// Noises for `seed` with the `noise_gradient` descriptors of `biomes`.
    /// Builders may only be run with biomes from this table.
    pub fn with_biomes(seed: i64, biomes: &[BiomeData]) -> Self {
        let mut gradients: Vec<GradientNoise> = Vec::new();
        for biome in biomes {
            for builder in [biome.surface, biome.subsurface].into_iter().flatten() {
                let Builder::NoiseGradient(g) = builder else {
                    continue;
                };
                if gradients.iter().any(|n| n.matches(&g)) {
                    continue;
                }
                let amplitudes: Vec<f64> = g.amplitudes.iter().map(|&a| a as f64).collect();
                gradients.push(GradientNoise {
                    name: g.noise_name,
                    first_octave: g.first_octave,
                    amplitudes: g.amplitudes,
                    noise: Normal::new(seed, g.noise_name, g.first_octave, &amplitudes),
                });
            }
        }
        let single = [1.0];
        let four = [1.0, 1.0, 1.0, 1.0];
        Self {
            surface: Normal::new(seed, "minecraft:surface", -2, &[1.0, 1.0, 1.0]),
            band_offset: Normal::new(seed, "minecraft:clay_bands_offset", 0, &single),
            bands: generate_bands(&mut named_rng(seed, "minecraft:clay_bands")),
            // Classic pillar noise: four octaves, highest one at the 1/4 input scale.
            pillar: Normal::new(seed, "minecraft:badlands_pillar", -3, &four),
            pillar_roof: Normal::new(seed, "minecraft:badlands_pillar_roof", 0, &single),
            iceberg: Normal::new(seed, "minecraft:iceberg_pillar", -3, &four),
            iceberg_roof: Normal::new(seed, "minecraft:iceberg_pillar_roof", 0, &single),
            biome_info: Normal::new(seed, "minecraft:biome_info", 0, &single),
            capped_floor: Normal::new(seed, "minecraft:capped_floor", 0, &single),
            capped_ceiling: Normal::new(seed, "minecraft:capped_ceiling", 0, &single),
            gradients,
        }
    }

    /// Surface noise at `(x * scale, z * scale)`, roughly in `[-1, 1]`.
    #[inline]
    pub(super) fn surface(&self, x: i32, z: i32, scale: f64) -> f64 {
        self.surface.at_2d(x as f64 * scale, z as f64 * scale)
    }

    /// Badlands band at a block: the 64-entry sequence shifted vertically by
    /// up to ±2 blocks by a slow (1/512) noise, as in vanilla (whose offset
    /// noise is a single simplex octave in `[-1, 1]`; ours is clamped to it).
    #[inline]
    pub(super) fn band(&self, x: i32, y: i32, z: i32) -> Band {
        let n = self
            .band_offset
            .at_2d(x as f64 / 512.0, z as f64 / 512.0)
            .clamp(-1.0, 1.0);
        let offset = (n * 2.0).round() as i32;
        self.bands[(y + offset).rem_euclid(BAND_COUNT as i32) as usize]
    }

    /// Eroded badlands pillar noise (classic 1/4 input scale).
    #[inline]
    pub(super) fn pillar(&self, x: i32, z: i32) -> f64 {
        self.pillar.at_2d(x as f64 * 0.25, z as f64 * 0.25)
    }

    /// Eroded badlands pillar roof noise (classic 1/512 input scale).
    #[inline]
    pub(super) fn pillar_roof(&self, x: i32, z: i32) -> f64 {
        self.pillar_roof.at_2d(x as f64 / 512.0, z as f64 / 512.0)
    }

    /// Iceberg noise (classic 0.1 input scale).
    #[inline]
    pub(super) fn iceberg(&self, x: i32, z: i32) -> f64 {
        self.iceberg.at_2d(x as f64 * 0.1, z as f64 * 0.1)
    }

    /// Iceberg roof noise (classic 1/10.24 input scale).
    #[inline]
    pub(super) fn iceberg_roof(&self, x: i32, z: i32) -> f64 {
        self.iceberg_roof
            .at_2d(x as f64 * 0.097_656_25, z as f64 * 0.097_656_25)
    }

    /// Measured native frozen-ocean warm patches: fixed MT3456 octave field
    /// plus MT2345 biome info. Keep the f32 weights/input scaling in order.
    pub(super) fn frozen_temperature(&self, x: i32, z: i32, base: f32) -> f32 {
        const WARM_PATCH: f32 = 0.2;
        const WEIGHT: f32 = 1.0 / 7.0;
        let (x, z) = (x as f32, z as f32);
        let (fx, fz) = (x * 0.05, z * 0.05);
        let a = FROZEN_TEMPERATURE[0].get_value_2d(fx, fz) * WEIGHT;
        let b = FROZEN_TEMPERATURE[1].get_value_2d(fx * 0.5, fz * 0.5) * (WEIGHT * 2.0);
        let c = FROZEN_TEMPERATURE[2].get_value_2d(fx * 0.25, fz * 0.25) * (WEIGHT * 4.0);
        let large = (a + b + c) * 7.0;
        let small = BIOME_INFO_NOISE.get_value_2d(x * 0.2, z * 0.2);
        if large + small < 0.3 && BIOME_INFO_NOISE.get_value_2d(x * 0.09, z * 0.09) < 0.8 {
            WARM_PATCH
        } else {
            base
        }
    }

    /// Swamp puddle noise (classic 1/4 input scale); puddles where positive.
    #[inline]
    pub(super) fn puddle(&self, x: i32, z: i32) -> f64 {
        self.biome_info.at_2d(x as f64 * 0.25, z as f64 * 0.25)
    }

    /// Nether beach patch noise (0.1 input scale).
    #[inline]
    pub(super) fn beach(&self, x: i32, z: i32) -> f64 {
        self.biome_info.at_2d(x as f64 * 0.1, z as f64 * 0.1)
    }

    /// Capped builder floor material noise, roughly `[-1, 1]`.
    #[inline]
    pub(super) fn capped_floor(&self, x: i32, z: i32) -> f64 {
        self.capped_floor
            .at_2d(x as f64 * CAPPED_SCALE, z as f64 * CAPPED_SCALE)
    }

    /// Capped builder ceiling material noise, roughly `[-1, 1]`.
    #[inline]
    pub(super) fn capped_ceiling(&self, x: i32, z: i32) -> f64 {
        self.capped_ceiling
            .at_2d(x as f64 * CAPPED_SCALE, z as f64 * CAPPED_SCALE)
    }

    /// 3D value of a `noise_gradient` descriptor at a block.
    ///
    /// # Panics
    /// When `g` is not part of the biome table these noises were built from
    /// (an integration error, not a data condition).
    pub(super) fn gradient(&self, g: &NoiseGradient, x: i32, y: i32, z: i32) -> f64 {
        let Some(n) = self.gradients.iter().find(|n| n.matches(g)) else {
            panic!(
                "noise `{}` was not built: SurfaceNoises must come from the biome table in use",
                g.noise_name
            );
        };
        n.noise.at(x as f64, y as f64, z as f64)
    }

    /// The badlands band sequence of this seed.
    #[cfg(test)]
    pub(super) fn bands(&self) -> &[Band; BAND_COUNT] {
        &self.bands
    }
}

/// Patch size of the capped builder's floor/ceiling materials. Not
/// observable from the data; basalt/blackstone and soul sand/soil patches in
/// BDS remain unmeasured: to confirm by BDS fixture.
const CAPPED_SCALE: f64 = 0.1;

/// Xoroshiro seeded from the world seed mixed with an FNV-1a hash of `name`,
/// so every noise gets an independent stream.
fn named_rng(seed: i64, name: &str) -> Xoroshiro128 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Xoroshiro128::from_seed(seed ^ hash as i64)
}

/// Vanilla badlands band sequence: terracotta base with an orange band every
/// 1-5 blocks, 2-5 yellow streaks (1-3 high), 2-5 brown (2-4), 2-5 red (1-3),
/// and 3-5 single white bands 4-19 blocks apart, each possibly flanked by
/// light gray. Later colours paint over earlier ones, as in vanilla.
fn generate_bands(rng: &mut Xoroshiro128) -> [Band; BAND_COUNT] {
    let mut bands = [Band::Base; BAND_COUNT];
    let mut i = 0;
    loop {
        i += rng.next_int(5) as usize + 1;
        if i >= BAND_COUNT {
            break;
        }
        bands[i] = Band::Orange;
        i += 1;
    }
    for (band, min_len) in [(Band::Yellow, 1), (Band::Brown, 2), (Band::Red, 1)] {
        let streaks = rng.next_int(4) + 2;
        for _ in 0..streaks {
            let len = rng.next_int(3) as usize + min_len;
            let start = rng.next_int(BAND_COUNT as u32) as usize;
            for slot in bands.iter_mut().skip(start).take(len) {
                *slot = band;
            }
        }
    }
    let whites = rng.next_int(3) + 3;
    let mut at = 0;
    for _ in 0..whites {
        at += rng.next_int(16) as usize + 4;
        if at >= BAND_COUNT {
            break;
        }
        bands[at] = Band::White;
        if at > 1 && rng.next_int(2) == 1 {
            bands[at - 1] = Band::LightGray;
        }
        if at < BAND_COUNT - 1 && rng.next_int(2) == 1 {
            bands[at + 1] = Band::LightGray;
        }
    }
    bands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_temperature_matches_bds_reference_water_and_ice_columns() {
        use crate::world::generator::bedrock::generated;
        use crate::world::generator::parity::Fixture;

        let fixture = Fixture::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/bds_1_26_51_seed_12345.json"),
        )
        .unwrap();
        let biome = generated::BIOMES
            .iter()
            .find(|biome| biome.identifier == "minecraft:frozen_ocean")
            .unwrap();
        let noises = SurfaceNoises::new(fixture.seed);
        let mut covered = [false; 2];
        for chunk in &fixture.chunks {
            for column in 0..256 {
                let block = chunk.surface_block[column].as_str();
                if chunk.surface_biome[column] != u32::from(biome.id)
                    || !matches!(block, "minecraft:water" | "minecraft:ice")
                {
                    continue;
                }
                let x = chunk.pos.x * 16 + (column % 16) as i32;
                let z = chunk.pos.z * 16 + (column / 16) as i32;
                let expected_ice = block == "minecraft:ice";
                covered[usize::from(expected_ice)] = true;
                assert_eq!(
                    super::super::local_temperature(&noises, biome, x, z) < 0.15,
                    expected_ice,
                    "native frozen-ocean surface at ({x}, {}, {z})",
                    chunk.surface[column]
                );
            }
        }
        assert_eq!(
            covered,
            [true, true],
            "the reference must exercise both sides of the freezing boundary"
        );
    }

    #[test]
    fn bands_are_deterministic_and_vary_by_seed() {
        let a = generate_bands(&mut named_rng(7, "minecraft:clay_bands"));
        let b = generate_bands(&mut named_rng(7, "minecraft:clay_bands"));
        assert_eq!(a, b);
        let differs =
            (8..16).any(|s| generate_bands(&mut named_rng(s, "minecraft:clay_bands")) != a);
        assert!(differs, "band sequences should depend on the seed");
    }

    #[test]
    fn bands_have_vanilla_composition() {
        let mut orange_total = 0;
        for seed in 0..32 {
            let bands = generate_bands(&mut named_rng(seed, "minecraft:clay_bands"));
            let count = |b: Band| bands.iter().filter(|&&x| x == b).count();
            assert!(count(Band::White) >= 1, "seed {seed}: no white band");
            assert!(count(Band::Yellow) >= 1 && count(Band::Brown) >= 2 && count(Band::Red) >= 1);
            assert!(count(Band::Orange) >= 1, "seed {seed}: no orange band");
            assert!(
                count(Band::Base) >= 10,
                "seed {seed}: base terracotta too rare"
            );
            orange_total += count(Band::Orange);
        }
        // An orange band every ~4 entries, partly painted over by the streaks:
        // a 64-entry sequence keeps on average well over 5.
        assert!(
            orange_total / 32 >= 6,
            "mean orange bands {}",
            orange_total / 32
        );
    }

    #[test]
    fn shifted_octaves_keep_values_in_range() {
        // first_octave 2 with 3 octaves is outside the seedable window.
        let n = Normal::new(1, "test", 2, &[1.0, 1.0, 1.0]);
        assert_eq!(n.coord_scale, 2f64.powi(4));
        let mut nonzero = false;
        for i in 0..64 {
            let v = n.at(i as f64 * 0.37, 1.5, i as f64 * -0.21);
            assert!(v.abs() < 2.0);
            nonzero |= v != 0.0;
        }
        assert!(nonzero);
    }

    #[test]
    fn surface_noise_is_bounded_and_varied() {
        let n = Normal::new(99, "minecraft:surface", -2, &[1.0, 1.0, 1.0]);
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for x in 0..256 {
            let v = n.at_2d(x as f64 * 4.0 * 0.0625, x as f64 * 3.0 * 0.0625);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo > -1.6 && hi < 1.6, "{lo}..{hi}");
        assert!(hi - lo > 0.3, "noise should vary: {lo}..{hi}");
    }
}
