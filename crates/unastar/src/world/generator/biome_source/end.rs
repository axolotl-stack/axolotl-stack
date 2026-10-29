//! The native End selector is not a multi-noise nearest-point search. Its erosion
//! input is supplied by the preset router; the central region and threshold
//! inclusivity are part of TheEndBiomeSource itself.

use unastar_noise::java_biomes::JavaBiome;

/// Select a native End biome from absolute quart X/Z and the router's erosion
/// function. The native source samples erosion at the containing chunk's center,
/// with Y fixed to zero; all quarts of that chunk use the same horizontal sample.
pub fn sample_end_biome(
    quart_x: i32,
    quart_z: i32,
    erosion: impl FnOnce(i32, i32) -> f64,
) -> JavaBiome {
    let chunk_x = quart_x >> 2;
    let chunk_z = quart_z >> 2;
    let x = i64::from(chunk_x);
    let z = i64::from(chunk_z);
    if x * x + z * z <= 4096 {
        return JavaBiome::TheEnd;
    }
    let center_x = chunk_x.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
    let center_z = chunk_z.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
    let value = erosion(center_x, center_z);
    if value > 0.25 {
        JavaBiome::EndHighlands
    } else if value >= -0.0625 {
        JavaBiome::EndMidlands
    } else if value < -0.21875 {
        JavaBiome::SmallEndIslands
    } else {
        // Preserve the native strict-less comparison: far-End NaN erosion
        // reaches barrens, not the complement of a greater-or-equal branch.
        JavaBiome::EndBarrens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Observed with actual Java1.21.11 TheEndBiomeSource and native constant
    // DensityFunctions, explicitly a selector primitive probe, not a vanilla
    // terrain reference. External primitive-end-end-boundary-seed12345.json.
    #[test]
    fn native_erosion_thresholds_keep_their_exact_inclusivity() {
        for (bits, expected) in [
            (0xbfcc_0000_0000_0001, JavaBiome::SmallEndIslands),
            (0xbfcc_0000_0000_0000, JavaBiome::EndBarrens),
            (0xbfb0_0000_0000_0001, JavaBiome::EndBarrens),
            (0xbfb0_0000_0000_0000, JavaBiome::EndMidlands),
            (0x3fd0_0000_0000_0000, JavaBiome::EndMidlands),
            (0x3fd0_0000_0000_0001, JavaBiome::EndHighlands),
        ] {
            assert_eq!(
                sample_end_biome(260, 0, |_, _| f64::from_bits(bits)),
                expected
            );
        }
    }

    #[test]
    fn native_center_uses_chunk_scaled_radius_before_erosion() {
        assert_eq!(sample_end_biome(259, 0, |_, _| -1.0), JavaBiome::TheEnd);
        assert_eq!(
            sample_end_biome(260, 0, |_, _| -1.0),
            JavaBiome::SmallEndIslands
        );
        assert_eq!(
            sample_end_biome(-260, 0, |_, _| -1.0),
            JavaBiome::SmallEndIslands
        );
    }

    #[test]
    fn native_far_end_nan_erosion_reaches_barrens() {
        // Both independent holdout NOISE captures select barrens throughout
        // chunk (999983,-999979), whose native island erosion is negative NaN.
        for seed in [987654321, -7046029254386353131] {
            let islands = unastar_noise::end_islands::EndIslands::new(seed);
            assert_eq!(
                sample_end_biome(3999932, -3999916, |x, z| islands.sample(x, z)),
                JavaBiome::EndBarrens,
            );
        }
    }
}
