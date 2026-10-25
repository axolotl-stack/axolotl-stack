//! `minecraft:swamp`: the overworld builder plus water puddles.
//!
//! Vanilla puddles: where the swamp noise (1/4 input scale) is positive and
//! the column's surface sits exactly at the water level (`sea_level - 1`),
//! the surface becomes water; the overworld builder then dresses the floor
//! below as a shallow underwater floor (mid material). Bedrock bounds the
//! puddle depth by `max_puddle_depth_below_sea_level` (1 in swamps, the
//! classic single-block puddle; 3 in mangrove swamps). How BDS spreads the
//! depth between 1 and that bound is not observable from the data: here it
//! grows by one block per 1/8 of noise (unmeasured: to confirm by BDS fixture).

use super::super::rng::BedrockRandom;
use super::{Column, Materials, SurfaceColumn, first_non_air, is_foundation, overworld};

/// Puddle depth gained per unit of puddle noise (noise 0..0.125 → 1 block,
/// 0.125..0.25 → 2, ...), capped by the biome's maximum.
const DEPTH_PER_NOISE: f64 = 8.0;

/// Run the swamp builder; returns the lowest y it dressed.
pub(super) fn build<C: SurfaceColumn>(
    col: &mut C,
    c: &Column<'_>,
    base: &Materials,
    max_puddle_depth: i32,
    rng: &mut BedrockRandom,
) -> Option<i32> {
    if c.surface && max_puddle_depth > 0 {
        puddle(col, c, base, max_puddle_depth);
    }
    overworld::build(col, c, base, rng)
}

fn puddle<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, base: &Materials, max_depth: i32) {
    let n = c.noises.puddle(c.x, c.z);
    if n <= 0.0 {
        return;
    }
    let Some((top, b)) = first_non_air(col, c.lo, c.hi) else {
        return;
    };
    if top != c.sea_level - 1 || !is_foundation(b, base.foundation) {
        return;
    }
    let depth = ((1.0 + n * DEPTH_PER_NOISE) as i32).min(max_depth);
    let sea = c.materials(base, top).sea;
    for y in (c.lo.max(top - depth + 1)..=top).rev() {
        if !is_foundation(col.get(y), base.foundation) {
            break;
        }
        col.set(y, sea);
    }
}

#[cfg(test)]
mod tests {
    use super::super::bb;
    use super::super::test_support::{PLAINS, SEA, VecColumn, biome, input};
    use super::super::{Builder, SurfaceNoises, build_column};
    use super::*;

    #[test]
    fn puddles_form_at_water_level_where_noise_is_positive() {
        let noises = SurfaceNoises::with_biomes(5, &[]);
        let b = biome(Builder::Swamp {
            materials: PLAINS,
            max_puddle_depth_below_sea_level: 3,
        });
        let (mut wet, mut dry) = (0, 0);
        for x in 0..256 {
            let mut col = VecColumn::terrain(SEA - 1, None);
            build_column(
                &mut col,
                &input(&b, x * 3, 0),
                &noises,
                &mut BedrockRandom::new(x as u32),
            );
            let n = noises.puddle(x * 3, 0);
            if n > 0.0 {
                wet += 1;
                assert_eq!(col.get(SEA - 1), bb::WATER, "x {x}");
                let depth = (SEA - 3..SEA).filter(|&y| col.get(y) == bb::WATER).count();
                assert!((1..=3).contains(&depth));
                // The floor below the puddle is a shallow underwater floor.
                let floor = SEA - 1 - depth as i32;
                assert_eq!(col.get(floor), bb::DIRT);
            } else {
                dry += 1;
                assert_eq!(col.get(SEA - 1), bb::GRASS_BLOCK, "x {x}");
            }
        }
        assert!(wet > 0 && dry > 0, "wet {wet} dry {dry}");
    }

    #[test]
    fn no_puddles_above_water_level() {
        let noises = SurfaceNoises::with_biomes(5, &[]);
        let b = biome(Builder::Swamp {
            materials: PLAINS,
            max_puddle_depth_below_sea_level: 1,
        });
        for x in 0..64 {
            let mut col = VecColumn::terrain(SEA + 2, None);
            build_column(
                &mut col,
                &input(&b, x * 3, 0),
                &noises,
                &mut BedrockRandom::new(1),
            );
            assert_eq!(col.count(|id| id == bb::WATER), 0);
        }
    }
}
