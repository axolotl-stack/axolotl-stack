//! `minecraft:noise_gradient` (sulfur caves): each solid block of the window
//! takes the block of the first `noise_block_specifiers` range (inclusive)
//! containing the descriptor's 3D noise at that block; blocks outside every
//! range, air, liquids and `non_replaceable_blocks` are left alone.
//!
//! The noise is the descriptor's normal noise (`first_octave`, `amplitudes`)
//! sampled at block coordinates, seeded per world from its name. Whether
//! BDS adds a vertical bias ("gradient") to the value is not observable from
//! the data; none is applied (statistical parity on the documented ranges).
//!
//! As a surface builder it dresses the surface layer (the top solid block
//! and the `depth` blocks below it); as a subsurface builder, its whole
//! window, so a biome using it for both covers the full column.

use super::super::generated::bb;
use super::{Column, NoiseGradient, SurfaceColumn, is_liquid};

/// Run the noise gradient builder; returns the lowest y of its window
/// (`None` in a surface pass over a column without solid blocks).
pub(super) fn build<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, g: &NoiseGradient) -> Option<i32> {
    let (lo, hi) = if c.surface {
        let (top, _) = (c.lo..c.hi)
            .rev()
            .map(|y| (y, col.get(y)))
            .find(|&(_, b)| !bb::air(b) && !is_liquid(b))?;
        ((top - c.depth.max(0)).max(c.lo), top + 1)
    } else {
        (c.lo, c.hi)
    };
    for y in lo..hi {
        let b = col.get(y);
        if bb::air(b) || is_liquid(b) || (g.non_replaceable)(b) {
            continue;
        }
        let n = c.noises.gradient(g, c.x, y, c.z);
        if let Some(&(_, _, block)) = g.specifiers.iter().find(|&&(min, max, _)| n >= min as f64 && n <= max as f64) {
            col.set(y, block);
        }
    }
    Some(lo)
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::test_support::{VecColumn, biome, input};
    use super::super::{BiomeData, Builder, SurfaceNoises, build_column, build_range};
    use super::*;

    fn is_bedrock(b: u32) -> bool {
        b == bb::BEDROCK
    }

    const EVERYWHERE: NoiseGradient = NoiseGradient {
        noise_name: "test:gradient",
        first_octave: -5,
        amplitudes: &[1.0, 0.0, 1.0],
        non_replaceable: is_bedrock,
        specifiers: &[(-10.0, 10.0, bb::CALCITE)],
    };

    const SPLIT: NoiseGradient = NoiseGradient {
        noise_name: "test:split",
        specifiers: &[(-10.0, 0.0, bb::CALCITE), (0.0, 10.0, bb::TUFF)],
        ..EVERYWHERE
    };

    fn cave_biome(g: NoiseGradient) -> BiomeData {
        BiomeData { subsurface: Some(Builder::NoiseGradient(g)), ..biome(Builder::NoiseGradient(g)) }
    }

    #[test]
    fn surface_and_subsurface_cover_the_column_except_non_replaceable() {
        let b = cave_biome(EVERYWHERE);
        let noises = SurfaceNoises::with_biomes(4, std::slice::from_ref(&b));
        let mut col = VecColumn::terrain(70, None);
        col.set(-64, bb::BEDROCK);
        for y in 0..10 {
            col.set(y, bb::AIR);
        }
        build_column(&mut col, &input(&b, 3, 3), &noises, &mut BedrockRandom::new(0));
        assert_eq!(col.get(-64), bb::BEDROCK);
        assert_eq!(col.count(|id| id == bb::STONE), 0);
        assert_eq!(col.count(|id| id == bb::AIR), 10 + (320 - 71));
        assert_eq!(col.count(|id| id == bb::CALCITE), (71 + 64 - 1 - 10) as usize);
    }

    #[test]
    fn ranges_split_by_noise_and_ranges_respect_bounds() {
        let b = cave_biome(SPLIT);
        let noises = SurfaceNoises::with_biomes(4, std::slice::from_ref(&b));
        let mut col = VecColumn::terrain(100, None);
        build_range(&mut col, &input(&b, 0, 0), &noises, &mut BedrockRandom::new(0), -40, 40);
        for y in -40..40 {
            let n = noises.gradient(&SPLIT, 0, y, 0);
            let expected = if n <= 0.0 { bb::CALCITE } else { bb::TUFF };
            assert_eq!(col.get(y), expected, "y {y}");
        }
        assert!((40..=100).chain(-64..-40).all(|y| col.get(y) == bb::STONE));
        assert!(col.count(|id| id == bb::CALCITE) > 0 && col.count(|id| id == bb::TUFF) > 0);
    }
}
