//! Core density function types for terrain generation.
//!
//! The NoiseRegistry is the only type kept here - all other types
//! are now provided by the unastar_noise crate.

use crate::world::generator::noise::{BlendedNoise, DoublePerlinNoise};
use crate::world::generator::xoroshiro::PositionalRandomFactory;
use std::simd::prelude::*;
use unastar_noise::features::random::LegacyRandom;

use unastar_noise::{NOISE_PARAMS, NoiseRef, NoiseSource, overworld_settings, random_hashes};

enum RouterRandomFactory {
    Xoroshiro(PositionalRandomFactory),
    Legacy(i64),
}

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
    /// End-only legacy initialization stays off the Overworld/Bedrock startup path.
    end_islands: std::sync::OnceLock<unastar_noise::end_islands::EndIslands>,
}

/// Borrowed native Java arithmetic, statically selected outside the material loop.
/// The registry's ordinary NoiseSource implementation retains calibrated math.
pub struct JavaNoiseSource<'a> {
    registry: &'a NoiseRegistry,
}

impl NoiseRegistry {
    /// Instantiate every noise for a seed the way Java's `RandomState` does: each noise is
    /// seeded from the world's positional factory by `fromHashOf("minecraft:<noise>")`,
    /// the blended terrain noise by `fromHashOf("minecraft:terrain")`.
    pub fn new(seed: i64) -> Self {
        let params = overworld_settings::BLENDED_NOISE;
        Self::build(
            seed,
            false,
            [
                params.xz_scale,
                params.y_scale,
                params.xz_factor,
                params.y_factor,
                params.smear_scale_multiplier,
            ],
            false,
        )
    }

    /// Seed a dimension router using its native RNG family and blended-noise scales.
    /// `blended` is xz_scale, y_scale, xz_factor, y_factor, smear_scale_multiplier.
    /// Sampling remains statically dispatched through the same numeric noise tables.
    pub fn with_router_settings(seed: i64, legacy_random_source: bool, blended: [f64; 5]) -> Self {
        Self::build(seed, legacy_random_source, blended, true)
    }

    fn build(seed: i64, legacy_random_source: bool, blended: [f64; 5], native: bool) -> Self {
        let base = if legacy_random_source {
            RouterRandomFactory::Legacy(LegacyRandom::new(seed).next_long())
        } else {
            RouterRandomFactory::Xoroshiro(PositionalRandomFactory::new(seed))
        };
        let mut noises = Vec::with_capacity(NoiseRef::COUNT);
        for (noise_ref, params) in NOISE_PARAMS.iter() {
            debug_assert_eq!(
                noises.len(),
                *noise_ref as usize,
                "NOISE_PARAMS out of order"
            );
            let noise = match &base {
                RouterRandomFactory::Xoroshiro(base) => {
                    let mut rng = base.from_hash(params.hash);
                    if native {
                        DoublePerlinNoise::new_java(
                            &mut rng,
                            params.amplitudes,
                            params.first_octave,
                        )
                    } else {
                        DoublePerlinNoise::new(&mut rng, params.amplitudes, params.first_octave)
                    }
                }
                RouterRandomFactory::Legacy(factory_seed) => match noise_ref {
                    NoiseRef::Temperature => {
                        DoublePerlinNoise::legacy_nether_biome(&mut LegacyRandom::new(seed))
                    }
                    NoiseRef::Vegetation => DoublePerlinNoise::legacy_nether_biome(
                        &mut LegacyRandom::new(seed.wrapping_add(1)),
                    ),
                    _ => {
                        let mut rng = LegacyRandom::new(
                            factory_seed ^ i64::from(noise_ref.legacy_name_hash()),
                        );
                        if *noise_ref == NoiseRef::Offset {
                            DoublePerlinNoise::new_legacy(&mut rng, &[0.0], 0)
                        } else {
                            DoublePerlinNoise::new_legacy(
                                &mut rng,
                                params.amplitudes,
                                params.first_octave,
                            )
                        }
                    }
                },
            };
            noises.push(noise);
        }

        let [xz_scale, y_scale, xz_factor, y_factor, smear] = blended;
        let blended_noise = match &base {
            RouterRandomFactory::Xoroshiro(base) => {
                let mut rng = base.from_hash(random_hashes::TERRAIN);
                BlendedNoise::new(&mut rng, xz_scale, y_scale, xz_factor, y_factor, smear)
            }
            RouterRandomFactory::Legacy(_) => {
                let mut rng = LegacyRandom::new(seed);
                BlendedNoise::new(&mut rng, xz_scale, y_scale, xz_factor, y_factor, smear)
            }
        };

        Self {
            noises,
            blended_noise,
            seed,
            end_islands: std::sync::OnceLock::new(),
        }
    }

    /// Select native Java scalar/SIMD arithmetic once, without hot-path dyn dispatch.
    #[inline]
    pub fn java_source(&self) -> JavaNoiseSource<'_> {
        JavaNoiseSource { registry: self }
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
    const NATIVE_JAVA: bool = false;

    #[inline]
    fn sample_beardifier(&self, _: i32, _: i32, _: i32) -> f64 {
        0.0
    }

    #[inline]
    fn sample(&self, noise_ref: NoiseRef, x: f64, y: f64, z: f64) -> f64 {
        self.get(noise_ref).sample(x, y, z)
    }

    #[inline]
    fn sample_4(&self, noise_ref: NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        self.get(noise_ref).sample_4(x, y, z)
    }

    #[inline]
    fn sample_end_islands(&self, block_x: i32, block_z: i32) -> f64 {
        self.end_islands
            .get_or_init(|| unastar_noise::end_islands::EndIslands::new(self.seed))
            .sample(block_x, block_z)
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

impl NoiseSource for JavaNoiseSource<'_> {
    const NATIVE_JAVA: bool = true;

    #[inline]
    fn sample_beardifier(&self, _: i32, _: i32, _: i32) -> f64 {
        0.0
    }

    #[inline]
    fn sample(&self, noise_ref: NoiseRef, x: f64, y: f64, z: f64) -> f64 {
        self.registry.get(noise_ref).sample_java(x, y, z)
    }

    #[inline]
    fn sample_4(&self, noise_ref: NoiseRef, x: f64x4, y: f64x4, z: f64x4) -> f64x4 {
        self.registry.get(noise_ref).sample_java_4(x, y, z)
    }

    #[inline]
    fn sample_end_islands(&self, block_x: i32, block_z: i32) -> f64 {
        self.registry.sample_end_islands(block_x, block_z)
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
        _smear: f64,
    ) -> f64 {
        self.registry.blended_noise.sample_java(x, y, z)
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
        _smear: f64,
    ) -> f64x4 {
        self.registry.blended_noise.sample_java_4(x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::{NoiseRegistry, NoiseSource};

    #[test]
    fn end_density_registry_uses_legacy_world_seed() {
        // Pinned Java 1.21.11 API bits from end_islands::tests; seed-sensitive
        // island edges distinguish the world seed from terrain/noise stream salts.
        for (seed, x, z, expected) in [
            (0, 0, 0, 0x3fe2_0000_0000_0000_u64),
            (0, 1024, 0, 0xbfea_9026_8000_0000),
            (-1, 1024, 0, 0xbfeb_0000_0000_0000),
            (0, -1032, 0, 0x3fc5_b52f_0000_0000),
            (0, 370728, 0, 0xfff8_0000_0000_0000),
        ] {
            let noises = NoiseRegistry::new(seed);
            assert_eq!(
                noises.sample_end_islands(x, z).to_bits(),
                expected,
                "native registry End density seed={seed} block=({x},{z})"
            );
        }
    }

    #[test]
    fn legacy_java_climate_matches_native_scalar_and_simd_bits() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            seed: i64,
            points: [Point; 4],
        }
        #[derive(serde::Deserialize)]
        struct Point {
            block: [i32; 3],
            temperature: String,
            vegetation: String,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/java_legacy_climate_bits.json",
        )))
        .unwrap();
        for case in fixture.cases {
            let registry = NoiseRegistry::with_router_settings(
                case.seed,
                true,
                [0.25, 0.375, 80.0, 60.0, 8.0],
            );
            let source = registry.java_source();
            let xs = case.points.each_ref().map(|p| f64::from(p.block[0]) * 0.25);
            let zs = case.points.each_ref().map(|p| f64::from(p.block[2]) * 0.25);
            for (noise, bits) in [
                (
                    unastar_noise::NoiseRef::Temperature,
                    case.points
                        .each_ref()
                        .map(|p| u64::from_str_radix(&p.temperature, 16).unwrap()),
                ),
                (
                    unastar_noise::NoiseRef::Vegetation,
                    case.points
                        .each_ref()
                        .map(|p| u64::from_str_radix(&p.vegetation, 16).unwrap()),
                ),
            ] {
                for lane in 0..4 {
                    assert_eq!(
                        source.sample(noise, xs[lane], 0.0, zs[lane]).to_bits(),
                        bits[lane],
                        "native scalar climate seed={} noise={noise:?} block={:?}",
                        case.seed,
                        case.points[lane].block
                    );
                }
                let vector = source.sample_4(
                    noise,
                    super::f64x4::from_array(xs),
                    super::f64x4::splat(0.0),
                    super::f64x4::from_array(zs),
                );
                assert_eq!(
                    vector.to_array().map(f64::to_bits),
                    bits,
                    "native SIMD climate seed={} noise={noise:?}",
                    case.seed
                );
            }
        }
    }

    #[test]
    fn java_blended_simd_preserves_scalar_bits_across_smear_and_height_boundaries() {
        for seed in [0_i64, -1, i64::MAX] {
            for parameters in [
                [0.25, 0.375, 80.0, 60.0, 8.0],
                [0.25, 0.25, 80.0, 160.0, 4.0],
            ] {
                let registry = NoiseRegistry::with_router_settings(seed, true, parameters);
                let source = registry.java_source();
                let [xz_scale, y_scale, xz_factor, y_factor, smear] = parameters;
                for (x, z) in [
                    (-29_999_999.0, 29_999_999.0),
                    (-1.0, 1.0),
                    (0.0, 0.0),
                    (1024.0, -1024.0),
                ] {
                    let ys = [-64.0, 63.0, 64.0, 127.0];
                    let expected = ys.map(|y| {
                        source
                            .sample_blended_noise(
                                x, y, z, xz_scale, y_scale, xz_factor, y_factor, smear,
                            )
                            .to_bits()
                    });
                    let actual = source.sample_blended_noise_4(
                        x,
                        super::f64x4::from_array(ys),
                        z,
                        xz_scale,
                        y_scale,
                        xz_factor,
                        y_factor,
                        smear,
                    );
                    assert_eq!(
                        actual.to_array().map(f64::to_bits),
                        expected,
                        "native blended SIMD seed={seed} block=({x},{z}) parameters={parameters:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn native_material_cache_preserves_measured_stone_boundaries() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                use unastar_noise::{
                    ColumnContextGrid, FlatCacheGrid, NATIVE_GEOMETRY, NativeCellSampler,
                };
                let p = unastar_noise::overworld_settings::BLENDED_NOISE;
                let registry = NoiseRegistry::with_router_settings(
                    0,
                    false,
                    [
                        p.xz_scale,
                        p.y_scale,
                        p.xz_factor,
                        p.y_factor,
                        p.smear_scale_multiplier,
                    ],
                );
                let source = registry.java_source();
                let flat = FlatCacheGrid::new_native(0, 0, &source);
                let columns = ColumnContextGrid::new_native(0, 0, &source, &flat);
                let mut sampler = NativeCellSampler::new(0, 0, &flat, &columns, &source);
                sampler.initialize();
                // Actual Java 1.21.11 seed0 empty-Beardifier material captures.
                // Interpolating the entire nonlinear root made these water/air.
                let cases = [
                    (0_i32, 62_i32, 13_i32, 0x3f42_6065_3818_26f9_u64),
                    (7, 63, 14, 0x3f20_0bd1_dded_487e),
                ];
                for cell_x in 0..=1 {
                    sampler.advance_cell_x(cell_x);
                    for (x, y, z, expected) in cases {
                        if x.div_euclid(NATIVE_GEOMETRY.cell_width) as usize != cell_x {
                            continue;
                        }
                        let cell_y = (y - NATIVE_GEOMETRY.min_y)
                            .div_euclid(NATIVE_GEOMETRY.cell_height)
                            as usize;
                        let cell_z = z.div_euclid(NATIVE_GEOMETRY.cell_width) as usize;
                        sampler.select_cell_yz(cell_y, cell_z);
                        sampler.update_y(y);
                        sampler.update_x(x);
                        let local_z = z.rem_euclid(NATIVE_GEOMETRY.cell_width) as usize;
                        let batch = local_z / 4 * 4;
                        let actual = sampler.densities_4z(batch).to_array()[local_z % 4].to_bits();
                        assert_eq!(actual, expected, "native material ({x},{y},{z})");
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn native_modern_router_uses_exact_octave_persistence() {
        use unastar_noise::{
            FunctionContext, compute_continents_native_uncached,
            compute_temperature_native_uncached,
        };
        let p = unastar_noise::overworld_settings::BLENDED_NOISE;
        let registry = NoiseRegistry::with_router_settings(
            0,
            false,
            [
                p.xz_scale,
                p.y_scale,
                p.xz_factor,
                p.y_factor,
                p.smear_scale_multiplier,
            ],
        );
        let source = registry.java_source();
        // Pinned native raw router fields: nine-octave continentalness exposes
        // rounded persistence; temperature's 1.5 amplitude exposes reassociation.
        let actual = [
            compute_continents_native_uncached(&FunctionContext::new(16384, 9, -16392), &source)
                .to_bits(),
            compute_temperature_native_uncached(&FunctionContext::new(16384, 8, -16388), &source)
                .to_bits(),
        ];
        assert_eq!(actual, [0x3fbb_4fa5_bced_103b, 0x3fe4_e671_c22a_3e5c]);

        // The native SIMD leaf must preserve the scalar multiplication order,
        // including temperature's non-power-of-two amplitude.
        let xs = [4096.0, 4096.0, 4097.0, 4097.0];
        let zs = [-4098.0, -4097.0, -4098.0, -4097.0];
        let scalar = std::array::from_fn(|lane| {
            source
                .sample(
                    unastar_noise::NoiseRef::Temperature,
                    xs[lane],
                    0.0,
                    zs[lane],
                )
                .to_bits()
        });
        let vector = source.sample_4(
            unastar_noise::NoiseRef::Temperature,
            super::f64x4::from_array(xs),
            super::f64x4::splat(0.0),
            super::f64x4::from_array(zs),
        );
        assert_eq!(vector.to_array().map(f64::to_bits), scalar);
    }
}
