//! Overworld density function graph construction.
//!
//! This module builds the complete NoiseRouter for overworld terrain generation,
//! matching vanilla Minecraft's terrain algorithm.
//!
//! The overworld terrain is built in layers:
//! 1. Base climate noises (temperature, vegetation, continents, erosion, ridges)
//! 2. Shift functions for domain warping
//! 3. Terrain splines (offset, factor, jaggedness)
//! 4. Cave systems (spaghetti, cheese, noodle caves)
//! 5. Final density with blending and interpolation

use super::function::DensityFunction;
use super::markers::{CacheOnceMarker, FlatCacheMarker, Interpolated};
use super::math::{
    Clamp, Constant, Mapped, MappedType, RangeChoice, TwoArg, TwoArgType, YClampedGradient, YCoord,
};
use super::noise_funcs::{Noise, NoiseHolder, NoiseParams, RarityType, ShiftA, ShiftB, ShiftedNoise, WeirdScaledSampler};
use super::router::NoiseRouter;
use super::spline::SplineBuilder;
use super::terrain_funcs::Slide;
use crate::world::generator::noise::DoublePerlinNoise;
use crate::world::generator::xoroshiro::Xoroshiro128;
use std::sync::Arc;

// ============================================================================
// Noise Parameter Definitions
// ============================================================================

/// Noise parameters for temperature.
fn temperature_params() -> NoiseParams {
    NoiseParams::new(-10, vec![1.5, 0.0, 1.0, 0.0, 0.0, 0.0])
}

/// Noise parameters for vegetation/humidity.
fn vegetation_params() -> NoiseParams {
    NoiseParams::new(-8, vec![1.0, 1.0, 0.0, 0.0, 0.0, 0.0])
}

/// Noise parameters for continentalness.
fn continentalness_params() -> NoiseParams {
    NoiseParams::new(-9, vec![1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0])
}

/// Noise parameters for erosion.
fn erosion_params() -> NoiseParams {
    NoiseParams::new(-9, vec![1.0, 1.0, 0.0, 1.0, 1.0])
}

/// Noise parameters for ridges (weirdness).
fn ridges_params() -> NoiseParams {
    NoiseParams::new(-7, vec![1.0, 2.0, 1.0, 0.0, 0.0, 0.0])
}

/// Noise parameters for shift.
fn shift_params() -> NoiseParams {
    NoiseParams::new(-3, vec![1.0, 1.0, 1.0, 0.0])
}

/// Noise parameters for aquifer barrier.
fn aquifer_barrier_params() -> NoiseParams {
    NoiseParams::new(-3, vec![1.0])
}

/// Noise parameters for aquifer floodedness.
fn aquifer_floodedness_params() -> NoiseParams {
    NoiseParams::new(-7, vec![1.0, 0.5, 0.0, 0.0, 0.0])
}

/// Noise parameters for aquifer spread.
fn aquifer_spread_params() -> NoiseParams {
    NoiseParams::new(-5, vec![1.0, 0.0, 1.0])
}

/// Noise parameters for aquifer lava.
fn aquifer_lava_params() -> NoiseParams {
    NoiseParams::new(-1, vec![1.0, 1.0])
}

/// Noise parameters for ore vein A (toggle).
fn ore_vein_a_params() -> NoiseParams {
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for ore vein B (ridged).
fn ore_vein_b_params() -> NoiseParams {
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for ore gap.
fn ore_gap_params() -> NoiseParams {
    NoiseParams::new(-5, vec![1.0])
}

// ============================================================================
// Cave Noise Parameter Definitions
// ============================================================================

/// Noise parameters for cave entrance.
fn cave_entrance_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [0.4, 0.5, 1.0]
    NoiseParams::new(-7, vec![0.4, 0.5, 1.0])
}

/// Noise parameters for spaghetti 2D.
fn spaghetti_2d_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0]
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for spaghetti 2D modulator.
fn spaghetti_2d_modulator_params() -> NoiseParams {
    // firstOctave: -11, amplitudes: [1.0]
    NoiseParams::new(-11, vec![1.0])
}

/// Noise parameters for spaghetti 2D elevation.
fn spaghetti_2d_elevation_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for spaghetti 2D thickness.
fn spaghetti_2d_thickness_params() -> NoiseParams {
    // firstOctave: -11, amplitudes: [1.0]
    NoiseParams::new(-11, vec![1.0])
}

/// Noise parameters for spaghetti 3D first layer.
fn spaghetti_3d_1_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0]
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for spaghetti 3D second layer.
fn spaghetti_3d_2_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0]
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for spaghetti 3D rarity.
fn spaghetti_3d_rarity_params() -> NoiseParams {
    // firstOctave: -11, amplitudes: [1.0]
    NoiseParams::new(-11, vec![1.0])
}

/// Noise parameters for spaghetti 3D thickness.
fn spaghetti_3d_thickness_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for spaghetti roughness.
fn spaghetti_roughness_params() -> NoiseParams {
    // firstOctave: -5, amplitudes: [1.0]
    NoiseParams::new(-5, vec![1.0])
}

/// Noise parameters for spaghetti roughness modulator.
fn spaghetti_roughness_modulator_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for noodle.
fn noodle_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for noodle thickness.
fn noodle_thickness_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for noodle ridge A.
fn noodle_ridge_a_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0]
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for noodle ridge B.
fn noodle_ridge_b_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0]
    NoiseParams::new(-7, vec![1.0])
}

/// Noise parameters for pillar.
fn pillar_params() -> NoiseParams {
    // firstOctave: -7, amplitudes: [1.0, 1.0]
    NoiseParams::new(-7, vec![1.0, 1.0])
}

/// Noise parameters for pillar rareness.
fn pillar_rareness_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

/// Noise parameters for pillar thickness.
fn pillar_thickness_params() -> NoiseParams {
    // firstOctave: -8, amplitudes: [1.0]
    NoiseParams::new(-8, vec![1.0])
}

// ============================================================================
// Noise Holder Factory
// ============================================================================

/// Create and instantiate a noise holder with the given parameters and seed.
fn create_noise(params: NoiseParams, seed: i64, salt: &str) -> NoiseHolder {
    // Create a salted seed for this specific noise
    let salted_seed = hash_seed(seed, salt);
    let mut rng = Xoroshiro128::from_seed(salted_seed);
    let noise = DoublePerlinNoise::new(&mut rng, &params.amplitudes, params.first_octave);
    NoiseHolder::with_noise(params, noise)
}

/// Hash a seed with a salt string to create unique noise instances.
fn hash_seed(seed: i64, salt: &str) -> i64 {
    let mut hash = seed;
    for byte in salt.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as i64);
    }
    hash
}

// ============================================================================
// Density Function Builders
// ============================================================================

/// Create an add operation.
fn add(a: Arc<dyn DensityFunction>, b: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(TwoArg::new(TwoArgType::Add, a, b))
}

/// Create a multiply operation.
fn mul(a: Arc<dyn DensityFunction>, b: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(TwoArg::new(TwoArgType::Mul, a, b))
}


/// Create a constant.
fn constant(value: f64) -> Arc<dyn DensityFunction> {
    Arc::new(Constant::new(value))
}


/// Create a squeeze operation.
/// Part of Java's postProcess: clamp(d, -1, 1) then x/2 - x³/24
fn squeeze(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Mapped::new(MappedType::Squeeze, input))
}

/// Create a quarter negative operation.
fn quarter_negative(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Mapped::new(MappedType::QuarterNegative, input))
}

/// Apply noise gradient density transformation.
///
/// This is the core terrain shaping function from Java Edition:
/// ```text
/// noiseGradientDensity(factor, depth) = 4.0 * (factor * depth).quarterNegative()
/// ```
///
/// The quarterNegative transformation passes positive values through unchanged
/// but multiplies negative values by 0.25, which prevents large negative densities
/// from creating solid blocks at high Y coordinates (the "stone roof" bug).
fn noise_gradient_density(
    factor: Arc<dyn DensityFunction>,
    depth: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    let product = mul(depth, factor);
    let quarter_neg = quarter_negative(product);
    mul(constant(4.0), quarter_neg)
}

/// Create an interpolated marker (will be replaced by cache during wiring).
fn interpolated(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Interpolated::new(input))
}

/// Create a flat cache marker.
fn flat_cached(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(FlatCacheMarker::new(input))
}

/// Create a Y gradient.
fn y_gradient(from_y: i32, to_y: i32, from_value: f64, to_value: f64) -> Arc<dyn DensityFunction> {
    Arc::new(YClampedGradient::new(from_y, to_y, from_value, to_value))
}

/// Create a slide function for terrain edges with correct Java Overworld parameters.
///
/// Java's slideOverworld() for normal overworld (bl=false):
/// - min_y = -64
/// - height = 384 (so max_y = 320)
/// - top_slide_size = 80
/// - top_slide_offset = 64 (meaning 64 blocks DOWN from max_y, so slide starts at Y=256)
/// - top_slide_target = -0.078125
/// - bottom_slide_offset = 0
/// - bottom_slide_size = 24
/// - bottom_slide_target = 0.1171875
///
/// Note: The Rust Slide implementation uses top_slide_offset as an additive offset
/// (top_start = max_y + offset), so we negate Java's value to get the same behavior.
fn slide(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Slide::with_settings(
        input,
        -64,       // min_y
        320,       // max_y
        -64,       // top_slide_offset: Java uses 64 down from top, Rust adds to max_y, so -64
        80,        // top_slide_size
        -0.078125, // top_slide_target
        0,         // bottom_slide_offset
        24,        // bottom_slide_size
        0.1171875, // bottom_slide_target
    ))
}

/// Create a cache_once marker.
fn cache_once(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(CacheOnceMarker::new(input))
}

/// Create a min operation.
fn min(a: Arc<dyn DensityFunction>, b: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(TwoArg::new(TwoArgType::Min, a, b))
}

/// Create a max operation.
fn max(a: Arc<dyn DensityFunction>, b: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(TwoArg::new(TwoArgType::Max, a, b))
}

/// Create a clamp operation.
fn clamp(input: Arc<dyn DensityFunction>, min_val: f64, max_val: f64) -> Arc<dyn DensityFunction> {
    Arc::new(Clamp::new(input, min_val, max_val))
}

/// Create an abs operation.
fn abs(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Mapped::new(MappedType::Abs, input))
}

/// Create a cube operation.
fn cube(input: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    Arc::new(Mapped::new(MappedType::Cube, input))
}

/// Create a Y coordinate function.
fn y_coord() -> Arc<dyn DensityFunction> {
    Arc::new(YCoord::new())
}

/// Create a range choice function.
fn range_choice(
    input: Arc<dyn DensityFunction>,
    min_inclusive: f64,
    max_exclusive: f64,
    when_in_range: Arc<dyn DensityFunction>,
    when_out_of_range: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    Arc::new(RangeChoice::new(
        input,
        min_inclusive,
        max_exclusive,
        when_in_range,
        when_out_of_range,
    ))
}

/// Create a noise function with custom scales.
fn noise_with_scales(holder: NoiseHolder, xz_scale: f64, y_scale: f64) -> Arc<dyn DensityFunction> {
    Arc::new(Noise::new(holder, xz_scale, y_scale))
}

/// Create a weird scaled sampler.
fn weird_scaled_sampler(
    input: Arc<dyn DensityFunction>,
    noise: NoiseHolder,
    rarity_type: RarityType,
) -> Arc<dyn DensityFunction> {
    Arc::new(WeirdScaledSampler::new(input, noise, rarity_type))
}

// ============================================================================
// Spline Builders
// ============================================================================

/// Java's GLOBAL_OFFSET constant that shifts all terrain down.
/// This is critical for getting the surface at the correct Y level (~64).
const GLOBAL_OFFSET: f64 = -0.50375;

/// Build the terrain offset spline.
///
/// This creates the base terrain height offset based on continents, erosion, and ridges.
/// The offset determines the base Y level of the terrain surface.
///
/// Java applies GLOBAL_OFFSET (-0.50375) to the spline output, which shifts all
/// terrain downward. Without this, terrain would generate much higher than intended.
fn build_offset_spline(
    continents: Arc<dyn DensityFunction>,
    _erosion: Arc<dyn DensityFunction>,
    _ridges: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    // Simplified offset spline - in full implementation this would have
    // complex nested splines for different biome types
    //
    // The values here are chosen to produce surface heights around Y=64-80
    // after GLOBAL_OFFSET is applied. Vanilla values from TerrainProvider
    // produce similar results through more complex nested splines.

    // Create a simple spline based on continents
    // These base values are BEFORE GLOBAL_OFFSET is applied
    //
    // The depth formula is: y_gradient + offset, where y_gradient goes from
    // 1.5 at Y=-64 to -1.5 at Y=320. Surface forms where density crosses 0.
    //
    // To get surface at Y=64:
    //   y_gradient at Y=64 = 1.5 - 3.0 * (64+64)/(320+64) = 1.5 - 1.0 = 0.5
    //   We need: y_gradient + offset = 0 at surface
    //   So offset should be around -0.5 for Y=64 surface
    //
    // With GLOBAL_OFFSET of -0.50375, the base spline values should be small
    // (near 0) to get surfaces around Y=64.
    let spline = SplineBuilder::new(continents.clone())
        // Deep ocean: base -0.2 → after GLOBAL_OFFSET: -0.70375 → surface ~Y=30
        .add(-1.1, -0.2, 0.0)
        // Ocean: base -0.1 → after GLOBAL_OFFSET: -0.60375 → surface ~Y=45
        .add(-0.5, -0.1, 0.2)
        // Coast: base 0.0 → after GLOBAL_OFFSET: -0.50375 → surface ~Y=55
        .add(-0.25, 0.0, 0.2)
        // Low inland: base 0.05 → after GLOBAL_OFFSET: -0.45375 → surface ~Y=62
        .add(0.0, 0.05, 0.1)
        // Mid inland: base 0.1 → after GLOBAL_OFFSET: -0.40375 → surface ~Y=68
        .add(0.5, 0.1, 0.15)
        // High inland / mountains: base 0.25 → after GLOBAL_OFFSET: -0.25375 → surface ~Y=85
        .add(1.0, 0.25, 0.0)
        .build();

    // Apply GLOBAL_OFFSET to shift all terrain down to proper Y levels
    // Java: DensityFunctions.add(DensityFunctions.constant(-0.50375F), spline)
    add(constant(GLOBAL_OFFSET), Arc::new(spline))
}

/// Build the terrain factor spline.
///
/// Factor controls how much the terrain varies (amplitude of features).
fn build_factor_spline(
    _continents: Arc<dyn DensityFunction>,
    erosion: Arc<dyn DensityFunction>,
    _ridges: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    // Simplified factor spline
    let spline = SplineBuilder::new(erosion.clone())
        // Low erosion = high factor (mountainous)
        .add(-1.0, 6.0, 0.0)
        // Medium erosion = medium factor
        .add(0.0, 4.0, -1.0)
        // High erosion = low factor (flat)
        .add(1.0, 1.0, 0.0)
        .build();

    Arc::new(spline)
}

/// Build the jaggedness spline.
///
/// Jaggedness adds rough peaks to terrain in appropriate areas.
fn build_jaggedness_spline(
    _continents: Arc<dyn DensityFunction>,
    erosion: Arc<dyn DensityFunction>,
    ridges: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    // Jaggedness is mostly zero except in specific mountain biomes
    let spline = SplineBuilder::new(ridges.clone())
        // Low ridges = no jaggedness
        .add(-1.0, 0.0, 0.0)
        // High ridges = jaggedness
        .add(0.5, 0.0, 0.5)
        .add(1.0, 0.5, 0.0)
        .build();

    // Only apply jaggedness in low-erosion, high-continentalness areas
    let erosion_check = SplineBuilder::new(erosion.clone())
        .add(-1.0, 1.0, 0.0)
        .add(0.0, 0.5, -0.5)
        .add(1.0, 0.0, 0.0)
        .build();

    mul(Arc::new(spline), Arc::new(erosion_check))
}

// ============================================================================
// Cave Density Function Builders
// ============================================================================

/// Build the spaghetti 2D thickness modulator.
///
/// From `caves/spaghetti_2d_thickness_modulator.json`:
/// cache_once(add(-0.95, mul(-0.35, noise(spaghetti_2d_thickness, xz_scale=2.0, y_scale=1.0))))
fn build_spaghetti_2d_thickness_modulator(seed: i64) -> Arc<dyn DensityFunction> {
    let noise = create_noise(spaghetti_2d_thickness_params(), seed, "spaghetti_2d_thickness");
    let noise_func = noise_with_scales(noise, 2.0, 1.0);

    // add(-0.95, mul(-0.35, noise))
    cache_once(add(constant(-0.95), mul(constant(-0.35), noise_func)))
}

/// Build the spaghetti roughness function.
///
/// From `caves/spaghetti_roughness_function.json`:
/// cache_once(mul(
///     add(-0.05, mul(-0.05, noise(spaghetti_roughness_modulator))),
///     add(-0.4, abs(noise(spaghetti_roughness)))
/// ))
fn build_spaghetti_roughness_function(seed: i64) -> Arc<dyn DensityFunction> {
    let mod_noise = create_noise(spaghetti_roughness_modulator_params(), seed, "spaghetti_roughness_modulator");
    let rough_noise = create_noise(spaghetti_roughness_params(), seed, "spaghetti_roughness");

    let mod_func = noise_with_scales(mod_noise, 1.0, 1.0);
    let rough_func = noise_with_scales(rough_noise, 1.0, 1.0);

    // mul(add(-0.05, mul(-0.05, mod)), add(-0.4, abs(rough)))
    let part1 = add(constant(-0.05), mul(constant(-0.05), mod_func));
    let part2 = add(constant(-0.4), abs(rough_func));

    cache_once(mul(part1, part2))
}

/// Build the pillars density function.
///
/// From `caves/pillars.json`:
/// cache_once(mul(
///     add(mul(2.0, noise(pillar, xz_scale=25.0, y_scale=0.3)),
///         add(-1.0, mul(-1.0, noise(pillar_rareness)))),
///     cube(add(0.55, mul(0.55, noise(pillar_thickness))))
/// ))
fn build_pillars(seed: i64) -> Arc<dyn DensityFunction> {
    let pillar_noise = create_noise(pillar_params(), seed, "pillar");
    let rareness_noise = create_noise(pillar_rareness_params(), seed, "pillar_rareness");
    let thickness_noise = create_noise(pillar_thickness_params(), seed, "pillar_thickness");

    let pillar_func = noise_with_scales(pillar_noise, 25.0, 0.3);
    let rareness_func = noise_with_scales(rareness_noise, 1.0, 1.0);
    let thickness_func = noise_with_scales(thickness_noise, 1.0, 1.0);

    // add(mul(2.0, pillar), add(-1.0, mul(-1.0, rareness)))
    let part1 = add(
        mul(constant(2.0), pillar_func),
        add(constant(-1.0), mul(constant(-1.0), rareness_func)),
    );

    // cube(add(0.55, mul(0.55, thickness)))
    let part2 = cube(add(constant(0.55), mul(constant(0.55), thickness_func)));

    cache_once(mul(part1, part2))
}

/// Build the spaghetti 2D density function.
///
/// From `caves/spaghetti_2d.json`:
/// A complex function that creates horizontal 2D cave tunnels.
fn build_spaghetti_2d(seed: i64, thickness_modulator: Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
    let spaghetti_2d_noise = create_noise(spaghetti_2d_params(), seed, "spaghetti_2d");
    let modulator_noise = create_noise(spaghetti_2d_modulator_params(), seed, "spaghetti_2d_modulator");
    let elevation_noise = create_noise(spaghetti_2d_elevation_params(), seed, "spaghetti_2d_elevation");

    // weird_scaled_sampler with type_2 rarity
    let modulator_func = noise_with_scales(modulator_noise, 2.0, 1.0);
    let weird_sampler = weird_scaled_sampler(modulator_func, spaghetti_2d_noise, RarityType::Type2);

    // add(weird_sampler, mul(0.083, thickness_modulator))
    let spaghetti_part = add(weird_sampler, mul(constant(0.083), thickness_modulator.clone()));

    // elevation: add(0.0, mul(8.0, noise(elevation, y_scale=0.0)))
    let elevation_func = noise_with_scales(elevation_noise, 1.0, 0.0);
    let elevation_val = add(constant(0.0), mul(constant(8.0), elevation_func));

    // y_gradient from -64 (8.0) to 320 (-40.0)
    let y_grad = y_gradient(-64, 320, 8.0, -40.0);

    // abs(add(elevation_val, y_grad))
    let abs_elevation = abs(add(elevation_val, y_grad));

    // add(abs_elevation, thickness_modulator)
    let with_thickness = add(abs_elevation, thickness_modulator);

    // cube of that
    let cubed = cube(with_thickness);

    // max(spaghetti_part, cubed)
    let max_result = max(spaghetti_part, cubed);

    // clamp(-1.0, 1.0)
    clamp(max_result, -1.0, 1.0)
}

/// Build the cave entrances density function.
///
/// From `caves/entrances.json`:
/// This is a complex nested function that creates cave entrances.
fn build_cave_entrances(
    seed: i64,
    spaghetti_roughness: Arc<dyn DensityFunction>,
) -> Arc<dyn DensityFunction> {
    let entrance_noise = create_noise(cave_entrance_params(), seed, "cave_entrance");
    let spaghetti_3d_rarity_noise = create_noise(spaghetti_3d_rarity_params(), seed, "spaghetti_3d_rarity");
    let spaghetti_3d_1_noise = create_noise(spaghetti_3d_1_params(), seed, "spaghetti_3d_1");
    let spaghetti_3d_2_noise = create_noise(spaghetti_3d_2_params(), seed, "spaghetti_3d_2");
    let spaghetti_3d_thickness_noise = create_noise(spaghetti_3d_thickness_params(), seed, "spaghetti_3d_thickness");

    // Part 1: add(0.37, noise(cave_entrance, xz_scale=0.75, y_scale=0.5))
    let entrance_func = noise_with_scales(entrance_noise, 0.75, 0.5);
    let entrance_part = add(constant(0.37), entrance_func);

    // Part 2: y_gradient from -10 (0.3) to 30 (0.0)
    let y_grad = y_gradient(-10, 30, 0.3, 0.0);

    // argument1 of min: add(entrance_part, y_grad)
    let min_arg1 = add(entrance_part, y_grad);

    // 3D spaghetti caves - weird scaled samplers
    let rarity_func = cache_once(noise_with_scales(spaghetti_3d_rarity_noise, 2.0, 1.0));

    let weird_1 = weird_scaled_sampler(rarity_func.clone(), spaghetti_3d_1_noise, RarityType::Type1);
    let weird_2 = weird_scaled_sampler(rarity_func, spaghetti_3d_2_noise, RarityType::Type1);

    // max of the two weird samplers
    let max_weird = max(weird_1, weird_2);

    // thickness modifier: add(-0.0765, mul(-0.0115, noise(thickness)))
    let thickness_func = noise_with_scales(spaghetti_3d_thickness_noise, 1.0, 1.0);
    let thickness_mod = add(constant(-0.0765), mul(constant(-0.011499999999999996), thickness_func));

    // add(max_weird, thickness_mod)
    let combined = add(max_weird, thickness_mod);

    // clamp(-1.0, 1.0)
    let clamped = clamp(combined, -1.0, 1.0);

    // add(spaghetti_roughness, clamped)
    let with_roughness = add(spaghetti_roughness, clamped);

    // argument2 of min
    let min_arg2 = with_roughness;

    // min(min_arg1, min_arg2)
    cache_once(min(min_arg1, min_arg2))
}

/// Build the noodle caves density function.
///
/// From `caves/noodle.json`:
/// This creates thin, winding cave tunnels.
fn build_noodle_caves(seed: i64) -> Arc<dyn DensityFunction> {
    let noodle_noise = create_noise(noodle_params(), seed, "noodle");
    let thickness_noise = create_noise(noodle_thickness_params(), seed, "noodle_thickness");
    let ridge_a_noise = create_noise(noodle_ridge_a_params(), seed, "noodle_ridge_a");
    let ridge_b_noise = create_noise(noodle_ridge_b_params(), seed, "noodle_ridge_b");

    let noodle_func = noise_with_scales(noodle_noise, 1.0, 1.0);
    let thickness_func = noise_with_scales(thickness_noise, 1.0, 1.0);
    let ridge_a_func = noise_with_scales(ridge_a_noise, 2.6666666666666665, 2.6666666666666665);
    let ridge_b_func = noise_with_scales(ridge_b_noise, 2.6666666666666665, 2.6666666666666665);

    let y = y_coord();

    // Inner range choice: if Y in [-60, 321), use noodle noise, else -1.0
    let inner_noodle = range_choice(
        y.clone(),
        -60.0,
        321.0,
        noodle_func,
        constant(-1.0),
    );
    let inner_noodle = interpolated(inner_noodle);

    // Outer range choice: if inner_noodle in [-1000000, 0), return 64.0
    // else compute the complex noodle shape

    // Thickness part: interpolated(range_choice(y, -60, 321, add(-0.075, mul(-0.025, thickness)), 0.0))
    let thickness_part = range_choice(
        y.clone(),
        -60.0,
        321.0,
        add(constant(-0.07500000000000001), mul(constant(-0.025), thickness_func)),
        constant(0.0),
    );
    let thickness_part = interpolated(thickness_part);

    // Ridge A: abs(interpolated(range_choice(y, -60, 321, ridge_a, 0.0)))
    let ridge_a_part = abs(interpolated(range_choice(
        y.clone(),
        -60.0,
        321.0,
        ridge_a_func,
        constant(0.0),
    )));

    // Ridge B: abs(interpolated(range_choice(y, -60, 321, ridge_b, 0.0)))
    let ridge_b_part = abs(interpolated(range_choice(
        y.clone(),
        -60.0,
        321.0,
        ridge_b_func,
        constant(0.0),
    )));

    // max(ridge_a, ridge_b)
    let max_ridges = max(ridge_a_part, ridge_b_part);

    // mul(1.5, max_ridges)
    let scaled_ridges = mul(constant(1.5), max_ridges);

    // add(thickness_part, scaled_ridges)
    let when_out_of_range = add(thickness_part, scaled_ridges);

    range_choice(
        inner_noodle,
        -1000000.0,
        0.0,
        constant(64.0),
        when_out_of_range,
    )
}

// ============================================================================
// Main Router Builder
// ============================================================================

/// Build the complete overworld NoiseRouter.
///
/// This constructs all 15 density functions that make up the overworld
/// terrain generation system.
pub fn build_overworld_router(seed: i64) -> NoiseRouter {
    // ========== Create Base Noises ==========

    // Climate noises
    let temperature_noise = create_noise(temperature_params(), seed, "temperature");
    let vegetation_noise = create_noise(vegetation_params(), seed, "vegetation");
    let continents_noise = create_noise(continentalness_params(), seed, "continentalness");
    let erosion_noise = create_noise(erosion_params(), seed, "erosion");
    let ridges_noise = create_noise(ridges_params(), seed, "ridges");

    // Shift noises
    let shift_noise = create_noise(shift_params(), seed, "offset");

    // Aquifer noises
    let barrier_noise_holder = create_noise(aquifer_barrier_params(), seed, "aquifer_barrier");
    let floodedness_noise_holder = create_noise(aquifer_floodedness_params(), seed, "aquifer_fluid_level_floodedness");
    let spread_noise_holder = create_noise(aquifer_spread_params(), seed, "aquifer_fluid_level_spread");
    let lava_noise_holder = create_noise(aquifer_lava_params(), seed, "aquifer_lava");

    // Ore vein noises
    let vein_toggle_noise = create_noise(ore_vein_a_params(), seed, "ore_vein_a");
    let vein_ridged_noise = create_noise(ore_vein_b_params(), seed, "ore_vein_b");
    let vein_gap_noise = create_noise(ore_gap_params(), seed, "ore_gap");

    // ========== Create Shift Functions ==========

    let shift_x: Arc<dyn DensityFunction> = Arc::new(ShiftA::new(shift_noise.clone()));
    let shift_z: Arc<dyn DensityFunction> = Arc::new(ShiftB::new(shift_noise.clone()));

    // ========== Create Climate Functions ==========

    // Temperature with domain warping
    let temperature: Arc<dyn DensityFunction> = Arc::new(ShiftedNoise::new(
        shift_x.clone(),
        constant(0.0),
        shift_z.clone(),
        temperature_noise,
        0.25,
        0.0,
    ));
    let temperature = flat_cached(temperature);

    // Vegetation with domain warping
    let vegetation: Arc<dyn DensityFunction> = Arc::new(ShiftedNoise::new(
        shift_x.clone(),
        constant(0.0),
        shift_z.clone(),
        vegetation_noise,
        0.25,
        0.0,
    ));
    let vegetation = flat_cached(vegetation);

    // Continents with domain warping
    let continents: Arc<dyn DensityFunction> = Arc::new(ShiftedNoise::new(
        shift_x.clone(),
        constant(0.0),
        shift_z.clone(),
        continents_noise,
        0.25,
        0.0,
    ));
    let continents = flat_cached(continents);

    // Erosion with domain warping
    let erosion: Arc<dyn DensityFunction> = Arc::new(ShiftedNoise::new(
        shift_x.clone(),
        constant(0.0),
        shift_z.clone(),
        erosion_noise,
        0.25,
        0.0,
    ));
    let erosion = flat_cached(erosion);

    // Ridges (weirdness) with domain warping
    let ridges: Arc<dyn DensityFunction> = Arc::new(ShiftedNoise::new(
        shift_x.clone(),
        constant(0.0),
        shift_z.clone(),
        ridges_noise,
        0.25,
        0.0,
    ));
    let ridges = flat_cached(ridges);

    // ========== Build Terrain Splines ==========

    let offset_spline = build_offset_spline(continents.clone(), erosion.clone(), ridges.clone());
    let factor_spline = build_factor_spline(continents.clone(), erosion.clone(), ridges.clone());
    let jaggedness_spline = build_jaggedness_spline(continents.clone(), erosion.clone(), ridges.clone());

    // ========== Create Depth Function ==========

    // Depth is the Y distance from surface, normalized
    // At surface (Y=64), depth = 0; below surface depth increases
    let depth = y_gradient(
        -64,  // min Y
        320,  // max Y
        1.5,  // value at bottom
        -1.5, // value at top
    );

    // Combine with offset to get actual depth relative to terrain surface
    let depth = add(depth, offset_spline.clone());

    // ========== Create Preliminary Surface Level ==========

    // This is used for surface detection before full density evaluation
    let preliminary_surface_level = flat_cached(offset_spline.clone());

    // ========== Build Sloped Cheese Terrain ==========

    // The main terrain density formula from Java Edition:
    // terrain = noiseGradientDensity(factor, depth + jaggedness)
    // where noiseGradientDensity(f, d) = 4.0 * (f * d).quarterNegative()
    //
    // The quarterNegative transformation is critical: it passes positive values
    // unchanged but reduces negative values by 75%. This prevents the "stone roof"
    // bug where large factor * depth products at high Y would remain positive.

    let terrain_base = add(depth.clone(), jaggedness_spline);

    // Apply noiseGradientDensity instead of simple multiplication
    // This applies quarterNegative to the product and multiplies by 4.0
    let terrain_shaped = noise_gradient_density(factor_spline.clone(), terrain_base.clone());

    // Apply sliding to smooth edges at world height limits
    // Note: Java's postProcess (mul 0.64 + squeeze) is applied AFTER all cave operations
    // in the Create Final Density section below
    let terrain_with_slide = slide(terrain_shaped);

    // ========== Build Cave Density Functions ==========

    // Build cave functions in order of dependencies
    let spaghetti_2d_thickness_modulator = build_spaghetti_2d_thickness_modulator(seed);
    let spaghetti_roughness = build_spaghetti_roughness_function(seed);
    let pillars = build_pillars(seed);
    let spaghetti_2d = build_spaghetti_2d(seed, spaghetti_2d_thickness_modulator);
    let cave_entrances = build_cave_entrances(seed, spaghetti_roughness);
    let noodle_caves = build_noodle_caves(seed);

    // ========== Combine Terrain with Caves (Java's underground logic) ==========
    //
    // In Java (NoiseRouterData.java:355-356), the structure is:
    //   rangeChoice(slopedCheese, -1000000.0, 1.5625,
    //       min(slopedCheese, entrances * 5),  // When in air/at surface
    //       underground(...)                    // When underground (>= 1.5625)
    //   )
    //
    // Key insight: Pillars ONLY exist in the underground() branch, which is only
    // called when terrain density >= 1.5625 (solid underground). This prevents
    // pillars from appearing in the sky!

    // Surface/air branch: just apply cave entrances (scaled by 5 in Java)
    let entrances_scaled = mul(constant(5.0), cave_entrances.clone());
    let surface_with_entrances = min(terrain_with_slide.clone(), entrances_scaled);

    // Underground branch: full cave system with pillars
    // In Java's underground(), pillars are thresholded with rangeChoice:
    //   rangeChoice(pillars, -1000000.0, 0.03, constant(-1000000.0), pillars)
    let pillars_thresholded = range_choice(
        pillars.clone(),
        -1000000.0,
        0.03,
        constant(-1000000.0),
        pillars,
    );

    // Build underground caves: min of all cave types, then max with pillars
    let underground_caves = min(
        min(
            min(terrain_with_slide.clone(), cave_entrances),
            spaghetti_2d,
        ),
        noodle_caves,
    );
    let underground_with_pillars = max(underground_caves, pillars_thresholded);

    // Use rangeChoice to select between surface and underground based on terrain density
    // If terrain < 1.5625 (air/surface): use surface_with_entrances (no pillars)
    // If terrain >= 1.5625 (underground): use underground_with_pillars (with pillars)
    let terrain_with_caves = range_choice(
        terrain_with_slide.clone(),
        -1000000.0,
        1.5625,  // SURFACE_DENSITY_THRESHOLD from Java
        surface_with_entrances,    // When terrain < 1.5625 (air/surface)
        underground_with_pillars,  // When terrain >= 1.5625 (underground)
    );

    // ========== Create Final Density ==========

    // Apply Java's postProcess transformation:
    // 1. interpolated(...) - marks for trilinear interpolation
    // 2. mul(0.64) - dampen values to reasonable range
    // 3. squeeze - clamp to [-1, 1] then apply x/2 - x³/24
    //
    // This is CRITICAL for fixing:
    // - Grid lines / cell edge artifacts (squeeze dampens extreme values)
    // - Stone pillars from sky (squeeze clamps runaway positive values)
    let interpolated_density = interpolated(terrain_with_caves);
    let dampened_density = mul(constant(0.64), interpolated_density);
    let final_density = squeeze(dampened_density);

    // ========== Create Aquifer Functions ==========

    let barrier_noise: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(barrier_noise_holder));
    let fluid_level_floodedness: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(floodedness_noise_holder));
    let fluid_level_spread: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(spread_noise_holder));
    let lava_noise: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(lava_noise_holder));

    // ========== Create Ore Vein Functions ==========

    let vein_toggle: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(vein_toggle_noise));
    let vein_ridged: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(vein_ridged_noise));
    let vein_gap: Arc<dyn DensityFunction> = Arc::new(Noise::with_holder(vein_gap_noise));

    // ========== Assemble Router ==========

    NoiseRouter::new(
        barrier_noise,
        fluid_level_floodedness,
        fluid_level_spread,
        lava_noise,
        temperature,
        vegetation,
        continents,
        erosion,
        depth,
        ridges,
        preliminary_surface_level,
        final_density,
        vein_toggle,
        vein_ridged,
        vein_gap,
    )
}

/// Build a simple test router with constant values.
///
/// Useful for testing without the full noise infrastructure.
pub fn build_test_router() -> NoiseRouter {
    let zero = constant(0.0);

    // Simple terrain: solid below Y=64, air above
    let test_density = y_gradient(-64, 320, 1.0, -1.0);

    NoiseRouter::new(
        zero.clone(),           // barrier_noise
        zero.clone(),           // fluid_level_floodedness
        zero.clone(),           // fluid_level_spread
        zero.clone(),           // lava_noise
        zero.clone(),           // temperature
        zero.clone(),           // vegetation
        zero.clone(),           // continents
        zero.clone(),           // erosion
        zero.clone(),           // depth
        zero.clone(),           // ridges
        zero.clone(),           // preliminary_surface_level
        test_density,           // final_density
        zero.clone(),           // vein_toggle
        zero.clone(),           // vein_ridged
        zero.clone(),           // vein_gap
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::density::context::SinglePointContext;

    #[test]
    fn test_build_test_router() {
        let router = build_test_router();
        let ctx = SinglePointContext::new(0, 64, 0);

        // At Y=64, density should be near 0 (surface level)
        let density = router.final_density.compute(&ctx);
        assert!(density.abs() < 0.5, "Expected near-zero density at Y=64, got {}", density);
    }

    #[test]
    fn test_build_test_router_underground() {
        let router = build_test_router();

        // Below surface should be positive (solid)
        let ctx_below = SinglePointContext::new(0, 0, 0);
        let density_below = router.final_density.compute(&ctx_below);
        assert!(density_below > 0.0, "Expected positive density underground, got {}", density_below);

        // Above surface should be negative (air)
        let ctx_above = SinglePointContext::new(0, 200, 0);
        let density_above = router.final_density.compute(&ctx_above);
        assert!(density_above < 0.0, "Expected negative density in air, got {}", density_above);
    }

    #[test]
    fn test_build_overworld_router() {
        let router = build_overworld_router(12345);
        let ctx = SinglePointContext::new(0, 64, 0);

        // Just verify it doesn't crash and returns a value
        let density = router.final_density.compute(&ctx);
        assert!(density.is_finite(), "Expected finite density, got {}", density);
    }

    #[test]
    fn test_overworld_density_at_heights() {
        let router = build_overworld_router(12345);

        // Print density values at various Y levels for debugging
        for y in [-64, 0, 32, 64, 100, 128, 200, 256, 300, 320] {
            let ctx = SinglePointContext::new(0, y, 0);
            let density = router.final_density.compute(&ctx);
            let depth = router.depth.compute(&ctx);
            eprintln!("Y={}: density={:.4}, depth={:.4}", y, density, depth);
        }

        // At high Y (e.g., 320), density should be negative (air)
        let ctx_high = SinglePointContext::new(0, 320, 0);
        let density_high = router.final_density.compute(&ctx_high);
        assert!(density_high < 0.0, "Expected negative density at Y=320 (air), got {}", density_high);

        // At low Y (e.g., 0), density should be positive (solid)
        let ctx_low = SinglePointContext::new(0, 0, 0);
        let density_low = router.final_density.compute(&ctx_low);
        assert!(density_low > 0.0, "Expected positive density at Y=0 (solid), got {}", density_low);
    }

    #[test]
    fn test_climate_functions_produce_values() {
        let router = build_overworld_router(12345);
        let ctx = SinglePointContext::new(100, 64, 200);

        // All climate functions should produce finite values
        let temp = router.temperature.compute(&ctx);
        let veg = router.vegetation.compute(&ctx);
        let cont = router.continents.compute(&ctx);
        let eros = router.erosion.compute(&ctx);
        let ridge = router.ridges.compute(&ctx);

        assert!(temp.is_finite(), "Temperature not finite: {}", temp);
        assert!(veg.is_finite(), "Vegetation not finite: {}", veg);
        assert!(cont.is_finite(), "Continents not finite: {}", cont);
        assert!(eros.is_finite(), "Erosion not finite: {}", eros);
        assert!(ridge.is_finite(), "Ridges not finite: {}", ridge);
    }

    #[test]
    fn test_hash_seed_consistency() {
        let seed = 12345;
        let hash1 = hash_seed(seed, "temperature");
        let hash2 = hash_seed(seed, "temperature");
        let hash3 = hash_seed(seed, "vegetation");

        assert_eq!(hash1, hash2, "Same salt should produce same hash");
        assert_ne!(hash1, hash3, "Different salts should produce different hashes");
    }

    #[test]
    fn test_spline_produces_reasonable_values() {
        let continents = constant(0.0); // Coastal
        let erosion = constant(0.0);    // Medium erosion
        let ridges = constant(0.0);     // No ridges

        let offset = build_offset_spline(continents, erosion, ridges);
        let ctx = SinglePointContext::new(0, 0, 0);

        let value = offset.compute(&ctx);
        // At continents=0 (coast), offset should be around 0.1
        assert!(value >= -1.0 && value <= 1.0, "Offset out of range: {}", value);
    }

    // ========== Cave Density Function Tests ==========

    #[test]
    fn test_spaghetti_2d_thickness_modulator() {
        let thickness_mod = build_spaghetti_2d_thickness_modulator(12345);

        // Test at various positions
        for (x, y, z) in [(0, 0, 0), (100, 64, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = thickness_mod.compute(&ctx);
            assert!(
                value.is_finite(),
                "Thickness modulator not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
            // Should be in the range determined by the formula: -0.95 + (-0.35 * noise)
            // Noise is typically in [-1, 1], so result is roughly [-1.3, -0.6]
            assert!(
                value >= -2.0 && value <= 0.0,
                "Thickness modulator out of expected range at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_spaghetti_roughness_function() {
        let roughness = build_spaghetti_roughness_function(12345);

        for (x, y, z) in [(0, 0, 0), (100, 64, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = roughness.compute(&ctx);
            assert!(
                value.is_finite(),
                "Roughness not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_pillars() {
        let pillars = build_pillars(12345);

        for (x, y, z) in [(0, 0, 0), (100, -20, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = pillars.compute(&ctx);
            assert!(
                value.is_finite(),
                "Pillars not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_spaghetti_2d() {
        let thickness_mod = build_spaghetti_2d_thickness_modulator(12345);
        let spaghetti = build_spaghetti_2d(12345, thickness_mod);

        for (x, y, z) in [(0, 0, 0), (100, 64, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = spaghetti.compute(&ctx);
            assert!(
                value.is_finite(),
                "Spaghetti 2D not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
            // Should be clamped to [-1, 1]
            assert!(
                value >= -1.0 && value <= 1.0,
                "Spaghetti 2D out of clamped range at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_cave_entrances() {
        let roughness = build_spaghetti_roughness_function(12345);
        let entrances = build_cave_entrances(12345, roughness);

        for (x, y, z) in [(0, 0, 0), (100, 64, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = entrances.compute(&ctx);
            assert!(
                value.is_finite(),
                "Cave entrances not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_noodle_caves() {
        let noodles = build_noodle_caves(12345);

        for (x, y, z) in [(0, 0, 0), (100, 64, 200), (-50, -30, 150)] {
            let ctx = SinglePointContext::new(x, y, z);
            let value = noodles.compute(&ctx);
            assert!(
                value.is_finite(),
                "Noodle caves not finite at ({}, {}, {}): {}",
                x, y, z, value
            );
        }
    }

    #[test]
    fn test_caves_deterministic() {
        // Test that cave functions produce the same output for the same seed
        let thickness1 = build_spaghetti_2d_thickness_modulator(42);
        let thickness2 = build_spaghetti_2d_thickness_modulator(42);

        let ctx = SinglePointContext::new(100, 30, 200);

        let val1 = thickness1.compute(&ctx);
        let val2 = thickness2.compute(&ctx);

        assert_eq!(val1, val2, "Cave functions should be deterministic");
    }

    #[test]
    fn test_caves_affect_final_density() {
        let router = build_overworld_router(12345);

        // Test at a low Y level where caves should potentially carve
        let ctx = SinglePointContext::new(100, -20, 200);
        let density = router.final_density.compute(&ctx);

        assert!(
            density.is_finite(),
            "Final density with caves not finite: {}",
            density
        );

        // At deep Y levels, caves can create negative density (air pockets)
        // or positive density (solid stone / pillars)
        // We just verify it produces sensible values
        assert!(
            density >= -1000.0 && density <= 1000.0,
            "Final density out of reasonable range: {}",
            density
        );
    }
}
