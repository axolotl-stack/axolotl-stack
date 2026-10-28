//! `minecraft:mesa`: badlands terracotta.
//!
//! Vanilla (classic badlands builders) from the top down, per run:
//! - the run is `depth + max(0, y - sea_level)` blocks deep, so high ground
//!   is terracotta far down (whole mesas are banded);
//! - top block above water:
//!   - `has_forest` plateaus above `86 + 2 depth`: coarse dirt or grass;
//!   - above `sea_level + 3 + depth`: orange terracotta above y 127,
//!     `hard_clay_material` where the surface noise's cosine term is
//!     positive, otherwise the band at that y;
//!   - lower: `top_material` (red sand), with `mid_material` below it;
//! - underwater: `sea_floor_material` on deep floors, `mid_material` on
//!   shallow ones (as the overworld builder);
//! - the rest of the run: the band at each y.
//!
//! Bands come from the per-seed 64-entry sequence ([`SurfaceNoises`]), whose
//! base is `hard_clay_material` and whose white bands are `clay_material`
//! (the Bedrock data names the two colours vanilla hard-codes).
//! `bryce_pillars` (eroded badlands) first raises spires of foundation from
//! the surface: where `e = min(|noise × 8.25|, 15 pillar)` is positive they
//! reach `sea_level + 1 + min(2.5 e², ceil(50 |roof|) + 14)`.
//!
//! [`SurfaceNoises`]: super::SurfaceNoises

use super::super::generated::bb;
use super::noises::Band;
use super::{
    Column, LEGACY_NOISE_SCALE, Materials, SurfaceColumn, convert_foundation, fill_sea,
    first_non_air, is_foundation, is_liquid, underwater,
};
use std::f64::consts::PI;

/// Above this y band tops are orange terracotta (vanilla).
const BAND_TOP: i32 = 127;
/// Wooded badlands plateaus start above `FOREST_BASE + 2 depth` (vanilla).
const FOREST_BASE: i32 = 86;

/// Mesa builder parameters.
#[derive(Debug, Clone, Copy)]
pub(super) struct Mesa {
    pub(super) materials: Materials,
    pub(super) clay: u32,
    pub(super) hard_clay: u32,
    pub(super) bryce_pillars: bool,
    pub(super) has_forest: bool,
}

/// Block of a band entry.
#[inline]
fn band_block(band: Band, p: &Mesa) -> u32 {
    match band {
        Band::Base => p.hard_clay,
        Band::White => p.clay,
        Band::Orange => bb::ORANGE_TERRACOTTA,
        Band::Yellow => bb::YELLOW_TERRACOTTA,
        Band::Brown => bb::BROWN_TERRACOTTA,
        Band::Red => bb::RED_TERRACOTTA,
        Band::LightGray => bb::LIGHT_GRAY_TERRACOTTA,
    }
}

/// Run the mesa builder; returns the lowest y it dressed.
pub(super) fn build<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, p: &Mesa) -> Option<i32> {
    let base = &p.materials;
    let sea = if c.biome.adjustments.is_empty() {
        base.sea
    } else {
        c.materials(base, c.sea_level - 1).sea
    };
    let mut hi = fill_sea(col, c, sea);
    if p.bryce_pillars && c.surface {
        hi = erode(col, c, base.foundation, hi);
    }
    // Vanilla's `cos(legacy_noise / 3 π) > 0`: the noise bands where plain
    // terracotta (and coarse dirt on plateaus) replaces the colours.
    let plain = (c.noise * LEGACY_NOISE_SCALE / 3.0 * PI).cos() > 0.0;
    let band = |y: i32| band_block(c.noises.band(c.x, y, c.z), p);
    let depth = c.depth.max(0);
    let mut m = *base;
    let mut remaining = -1;
    let mut under_sand = false;
    let mut started = false;
    let mut lowest = None;
    for y in (c.lo..hi).rev() {
        if c.topmost_only && started && remaining == 0 {
            break;
        }
        let b = col.get(y);
        if bb::air(b) {
            remaining = -1;
            continue;
        }
        if is_liquid(b) || !is_foundation(b, m.foundation) {
            continue;
        }
        if remaining == -1 {
            started = true;
            under_sand = false;
            m = c.materials(base, y);
            remaining = depth + (y - c.sea_level).max(0);
            let top = if underwater(col, y, c.sea_level) {
                if y < c.sea_level - m.sea_floor_depth - depth {
                    m.sea_floor
                } else {
                    m.mid
                }
            } else if let Some(steep) = c.steep {
                steep
            } else if p.has_forest && y > FOREST_BASE + depth * 2 {
                if plain {
                    bb::COARSE_DIRT
                } else {
                    bb::GRASS_BLOCK
                }
            } else if y > c.sea_level + 3 + depth {
                if y > BAND_TOP {
                    bb::ORANGE_TERRACOTTA
                } else if plain {
                    p.hard_clay
                } else {
                    band(y)
                }
            } else {
                under_sand = true;
                m.top
            };
            col.set(y, top);
            lowest = Some(y);
        } else if remaining > 0 {
            remaining -= 1;
            col.set(y, if under_sand { m.mid } else { band(y) });
            lowest = Some(y);
        }
    }
    convert_foundation(col, c.lo, c.hi, m.foundation);
    lowest
}

/// Height (exclusive) of the eroded badlands spire at the column, if any.
fn pillar_top(c: &Column<'_>) -> Option<i32> {
    let e = (c.noise * LEGACY_NOISE_SCALE)
        .abs()
        .min(c.noises.pillar(c.x, c.z) * 15.0);
    if e <= 0.0 {
        return None;
    }
    let roof = c.noises.pillar_roof(c.x, c.z).abs();
    let height = (e * e * 2.5).min((roof * 50.0).ceil() + 14.0);
    Some(c.sea_level + 1 + height as i32)
}

/// Raise the spire: fill the air between the surface and the spire top with
/// foundation (not over water, as in vanilla).
/// Keep an occupied-height bound through this phase so the dressing scan sees
/// any new spire without traversing the empty sky again.
fn erode<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, foundation: u32, occupied_hi: i32) -> i32 {
    let Some(top) = pillar_top(c) else {
        return occupied_hi;
    };
    let top = top.min(c.hi);
    let Some((surface, b)) = first_non_air(col, c.lo, occupied_hi) else {
        return occupied_hi;
    };
    if is_liquid(b) {
        return occupied_hi;
    }
    for y in surface + 1..top {
        col.set(y, foundation);
    }
    occupied_hi.max(top)
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::test_support::{PLAINS, SEA, VecColumn, biome, input};
    use super::super::{Builder, SurfaceNoises, build_column};
    use super::*;

    const BADLANDS: Materials = Materials {
        top: bb::RED_SAND,
        mid: bb::HARDENED_CLAY,
        ..PLAINS
    };

    fn mesa(bryce_pillars: bool, has_forest: bool) -> Builder {
        Builder::Mesa {
            materials: BADLANDS,
            clay: bb::WHITE_TERRACOTTA,
            hard_clay: bb::HARDENED_CLAY,
            bryce_pillars,
            has_forest,
        }
    }

    #[test]
    fn high_ground_is_terracotta_bands() {
        let noises = SurfaceNoises::with_biomes(21, &[]);
        let b = biome(mesa(false, false));
        for x in 0..32 {
            let mut col = VecColumn::terrain(110, None);
            build_column(
                &mut col,
                &input(&b, x * 11, 0),
                &noises,
                &mut BedrockRandom::new(x as u32),
            );
            // 110 - 63 = 47 extra blocks: the whole top of the hill is banded.
            for y in 110 - 47..=110 {
                assert!(bb::terracotta(col.get(y)), "x {x} y {y}: {}", col.get(y));
            }
            assert_eq!(col.get(111), bb::AIR);
            assert_eq!(
                col.count(|id| id == bb::RED_SAND || id == bb::GRASS_BLOCK),
                0
            );
        }
    }

    #[test]
    fn bands_follow_the_seed_sequence() {
        let noises = SurfaceNoises::with_biomes(21, &[]);
        let again = SurfaceNoises::with_biomes(21, &[]);
        let p = Mesa {
            materials: BADLANDS,
            clay: bb::WHITE_TERRACOTTA,
            hard_clay: bb::HARDENED_CLAY,
            bryce_pillars: false,
            has_forest: false,
        };
        let mut colours = 0;
        for y in 64..128 {
            let block = band_block(noises.band(5, y, 5), &p);
            assert_eq!(block, band_block(again.band(5, y, 5), &p));
            assert!(bb::terracotta(block));
            colours += usize::from(block != bb::HARDENED_CLAY);
        }
        assert!(colours >= 10, "bands should be mostly coloured stripes");
        // The offset noise shifts bands by at most 2 blocks.
        let idx = |y: i32| (y.rem_euclid(64)) as usize;
        for y in 64..128 {
            let b = noises.band(5, y, 5);
            assert!((-2..=2).any(|d| noises.bands()[idx(y + d)] == b));
        }
    }

    #[test]
    fn low_ground_is_red_sand_over_mid() {
        let noises = SurfaceNoises::with_biomes(21, &[]);
        let b = biome(mesa(false, false));
        let mut col = VecColumn::terrain(SEA + 1, None);
        build_column(
            &mut col,
            &input(&b, 0, 0),
            &noises,
            &mut BedrockRandom::new(0),
        );
        assert_eq!(col.get(SEA + 1), bb::RED_SAND);
        assert_eq!(col.get(SEA), bb::HARDENED_CLAY);
    }

    #[test]
    fn wooded_plateaus_get_grass_or_coarse_dirt() {
        let noises = SurfaceNoises::with_biomes(21, &[]);
        let b = biome(mesa(false, true));
        for x in 0..16 {
            let mut col = VecColumn::terrain(110, None);
            build_column(
                &mut col,
                &input(&b, x * 13, 0),
                &noises,
                &mut BedrockRandom::new(0),
            );
            let top = col.get(110);
            assert!(top == bb::GRASS_BLOCK || top == bb::COARSE_DIRT, "x {x}");
            assert!(bb::terracotta(col.get(109)));
        }
    }

    #[test]
    fn eroded_badlands_raise_banded_spires() {
        let noises = SurfaceNoises::with_biomes(21, &[]);
        let b = biome(mesa(true, false));
        let mut seen = 0;
        for i in 0..2048 {
            let (x, z) = (i % 64 * 3, i / 64 * 3);
            let inp = input(&b, x, z);
            let mut rng = BedrockRandom::new(0);
            let c = Column::new(&inp, &noises, &mut rng, -64, 320, true);
            let Some(top) = pillar_top(&c) else { continue };
            // Spire tops above the red sand belt are banded.
            if top - 1 <= SEA + 3 + c.depth.max(0) {
                continue;
            }
            let mut col = VecColumn::terrain(SEA + 1, None);
            build_column(&mut col, &inp, &noises, &mut BedrockRandom::new(0));
            assert!(
                (SEA + 1..top).all(|y| bb::terracotta(col.get(y))),
                "spire below {top}"
            );
            assert_eq!(col.get(top), bb::AIR);
            seen += 1;
            if seen == 8 {
                break;
            }
        }
        assert!(seen > 0, "no spire in the sampled area");
    }
}
