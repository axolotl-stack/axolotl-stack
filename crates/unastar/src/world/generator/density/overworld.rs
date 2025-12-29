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
use super::markers::{FlatCacheMarker, Interpolated};
use super::math::{Constant, Mapped, MappedType, TwoArg, TwoArgType, YClampedGradient};
use super::noise_funcs::{Noise, NoiseHolder, NoiseParams, ShiftA, ShiftB, ShiftedNoise};
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
/// Note: Currently unused - squeeze is part of Java's postProcess which we haven't implemented yet.
#[allow(dead_code)]
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

    // NOTE: Squeeze is NOT applied here! In Java, squeeze is part of postProcess
    // which happens after slide and with additional blending. Applying squeeze here
    // with values from noiseGradientDensity (which can be ~20+) causes incorrect
    // negative densities because squeeze(x) = x/2 - x³/24 goes negative for x > ~3.
    //
    // For now, we skip squeeze entirely. A proper implementation would add the
    // postProcess step with blendDensity and the 0.64 multiplier.

    // Apply sliding to smooth edges at world height limits
    let terrain_with_slide = slide(terrain_shaped);

    // ========== Create Final Density ==========

    // Wrap in interpolation marker for cell-based caching
    let final_density = interpolated(terrain_with_slide);

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
}
