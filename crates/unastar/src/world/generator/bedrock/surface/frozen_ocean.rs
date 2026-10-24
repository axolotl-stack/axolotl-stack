//! `minecraft:frozen_ocean`: the overworld builder plus icebergs.
//!
//! Vanilla iceberg shape (classic frozen ocean builder): where both the
//! surface noise (|noise × 8.25|) and the iceberg noise (× 15) exceed 1.8,
//! their minimum `e` gives a spire of height `min(1.2 e², ceil(40 |roof|) + 14)`
//! above sea level (2 lower where the local temperature is above 0.1), reaching
//! `height + 7` below it. Packed ice fills the air below the top (99%) and
//! the water above the bottom (85%), the random holes giving the ragged look;
//! the first `2 + rng(4)` (+1) packed ice blocks from the top above
//! `sea_level + 18 + rng(10)` become snow blocks. The surface ice sheet is
//! the shared top-water freezing of frozen biomes ([`super::freezes`]), with
//! vanilla's warm open-water patches.

use super::super::generated::bb;
use super::super::rng::BedrockRandom;
use super::{Column, LEGACY_NOISE_SCALE, Materials, SurfaceColumn, overworld};

/// Run the frozen ocean builder; returns the lowest y it dressed.
pub(super) fn build<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, base: &Materials, rng: &mut BedrockRandom) -> Option<i32> {
    if c.surface
        && let Some(berg) = iceberg(c)
    {
        place(col, c, berg, rng);
    }
    overworld::build(col, c, base, rng)
}

/// Vertical extent of an iceberg column: packed ice replaces air below
/// `top` and water above `bottom` (both exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Iceberg {
    top: i32,
    bottom: i32,
}

/// Iceberg extent at the column, if one rises here.
fn iceberg(c: &Column<'_>) -> Option<Iceberg> {
    let e = (c.noise * LEGACY_NOISE_SCALE).abs().min(c.noises.iceberg(c.x, c.z) * 15.0);
    if e <= 1.8 {
        return None;
    }
    let roof = c.noises.iceberg_roof(c.x, c.z).abs();
    let mut height = (e * e * 1.2).min((roof * 40.0).ceil() + 14.0);
    if c.temperature > 0.1 {
        height -= 2.0;
    }
    if height <= 2.0 {
        return None;
    }
    let sea = c.sea_level as f64;
    Some(Iceberg { top: (sea + height) as i32, bottom: (sea - height - 7.0) as i32 })
}

/// Build the iceberg from the top down through the open column, stopping at
/// the sea floor.
fn place<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, berg: Iceberg, rng: &mut BedrockRandom) {
    let max_snow = 2 + rng.next_int(4);
    let min_snow_y = c.sea_level + 18 + rng.next_int(10);
    let mut snow = 0;
    let lo = c.lo.max(berg.bottom + 1);
    let hi = c.hi.min(berg.top);
    for y in (lo..hi).rev() {
        let b = col.get(y);
        let ice = if bb::air(b) {
            rng.next_float() > 0.01
        } else if bb::water(b) {
            y < c.sea_level && rng.next_float() > 0.15
        } else {
            break;
        };
        if !ice {
            continue;
        }
        if snow <= max_snow && y > min_snow_y {
            col.set(y, bb::SNOW_BLOCK);
            snow += 1;
        } else {
            col.set(y, bb::PACKED_ICE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{PLAINS, SEA, VecColumn, biome, input};
    use super::super::{Builder, SurfaceNoises};
    use super::*;

    #[test]
    fn icebergs_rise_from_the_sea() {
        let noises = SurfaceNoises::with_biomes(11, &[]);
        let b = biome(Builder::FrozenOcean(PLAINS));
        let mut seen = 0;
        for i in 0..4096 {
            let (x, z) = (i % 64 * 5, i / 64 * 5);
            let inp = input(&b, x, z);
            let mut rng = BedrockRandom::new(i as u32);
            let c = Column::new(&inp, &noises, &mut rng, -64, 320, true);
            let Some(berg) = iceberg(&c) else { continue };
            // `iceberg` keeps heights above 2, truncated to whole blocks.
            assert!(berg.top >= SEA + 2 && berg.top < 320);
            assert!(berg.bottom < SEA - 9);
            let mut col = VecColumn::terrain(20, Some(SEA - 1));
            build(&mut col, &c, &PLAINS, &mut rng);
            // Only ice, snow, water and air above the sea floor.
            assert!((21..320).all(|y| {
                let id = col.get(y);
                id == bb::PACKED_ICE || id == bb::SNOW_BLOCK || id == bb::WATER || id == bb::AIR
            }));
            let above_sea = (SEA..berg.top).filter(|&y| col.get(y) != bb::AIR).count() as i32;
            assert!(above_sea * 10 >= (berg.top - SEA) * 8, "iceberg should be mostly solid above sea");
            seen += 1;
            if seen == 8 {
                break;
            }
        }
        assert!(seen > 0, "no iceberg in the sampled area");
    }
}
