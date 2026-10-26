//! `minecraft:overworld`: the classic layered surface.
//!
//! From the top down, the first foundation block below air starts a run:
//! - above water: `top_material`, then `depth` blocks of `mid_material`
//!   (both the steep material on flagged steep slopes);
//! - underwater (covered by liquid, or below the water surface level):
//!   - deep floors, below `sea_level - sea_floor_depth - depth`:
//!     `sea_floor_material` for the top and the `depth` blocks below;
//!   - shallow floors: `mid_material` (grass never generates underwater);
//! - a sand mid layer is followed by `rng(4) + max(0, y - sea_level)`
//!   blocks of sandstone (red sand: red sandstone) when `depth > 1`.
//!
//! `sea_floor_depth` is the depth below the water level where the sea floor
//! starts (Mojang's documentation), which is the classic builders' constant 7
//! that every non-warm ocean still uses. Below the surface layer the terrain
//! stone becomes `foundation_material`. Overworld biomes only surface the
//! topmost run (noise caves keep their stone); nether biomes surface every
//! floor, as the classic nether builder did.

use super::super::generated::bb;
use super::super::rng::BedrockRandom;
use super::{
    Column, Materials, SurfaceColumn, convert_foundation, fill_sea, is_foundation, is_liquid,
    underwater,
};

/// Run the overworld builder; returns the lowest y it dressed.
pub(super) fn build<C: SurfaceColumn>(
    col: &mut C,
    c: &Column<'_>,
    base: &Materials,
    rng: &mut BedrockRandom,
) -> Option<i32> {
    let sea = if c.biome.adjustments.is_empty() {
        base.sea
    } else {
        c.materials(base, c.sea_level - 1).sea
    };
    let hi = fill_sea(col, c, sea);
    let depth = c.depth.max(0);
    let mut foundation = base.foundation;
    // -1: the next foundation block starts a run; > 0: mid blocks left.
    let mut remaining = -1;
    let mut mid = base.mid;
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
        // Liquids keep the run state (the floor below is underwater); other
        // non-foundation blocks (ice, bedrock, deepslate) are skipped.
        if is_liquid(b) || !is_foundation(b, foundation) {
            continue;
        }
        if remaining == -1 {
            started = true;
            let m = c.materials(base, y);
            foundation = m.foundation;
            let (top, layer) = if underwater(col, y, c.sea_level) {
                if y < c.sea_level - m.sea_floor_depth - depth {
                    (m.sea_floor, m.sea_floor)
                } else {
                    (m.mid, m.mid)
                }
            } else if let Some(steep) = c.steep {
                (steep, steep)
            } else {
                (m.top, m.mid)
            };
            col.set(y, top);
            mid = layer;
            remaining = depth;
            lowest = Some(y);
        } else if remaining > 0 {
            remaining -= 1;
            col.set(y, mid);
            lowest = Some(y);
            if remaining == 0 && depth > 1 {
                let stone = if mid == bb::SAND {
                    Some(bb::SANDSTONE)
                } else if mid == bb::RED_SAND {
                    Some(bb::RED_SANDSTONE)
                } else {
                    None
                };
                if let Some(stone) = stone {
                    remaining = rng.next_int(4) + (y - c.sea_level).max(0);
                    mid = stone;
                }
            }
        }
    }
    convert_foundation(col, c.lo, c.hi, foundation);
    lowest
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{PLAINS, VecColumn, biome, input};
    use super::super::{Builder, Materials, SurfaceNoises, build_column};
    use super::*;

    #[test]
    fn sand_layer_rests_on_sandstone() {
        let noises = SurfaceNoises::with_biomes(3, &[]);
        let desert = Materials {
            top: bb::SAND,
            mid: bb::SAND,
            ..PLAINS
        };
        let b = biome(Builder::Overworld(desert));
        let mut found = false;
        for x in 0..64 {
            let mut col = VecColumn::terrain(80, None);
            build_column(
                &mut col,
                &input(&b, x * 7, 0),
                &noises,
                &mut BedrockRandom::new(x as u32),
            );
            assert_eq!(col.get(80), bb::SAND);
            let sand = (col.min_y..80)
                .rev()
                .take_while(|&y| col.get(y) == bb::SAND)
                .count() as i32;
            let below = col.get(79 - sand);
            if sand > 1 {
                // Depth > 1: at least one sandstone block (80 - 63 = 17 extra).
                assert_eq!(below, bb::SANDSTONE, "x {x}");
                found = true;
            }
            assert_eq!(col.count(|id| id == bb::GRASS_BLOCK || id == bb::DIRT), 0);
        }
        assert!(found, "no column had a sand layer deeper than one block");
    }

    #[test]
    fn only_the_topmost_run_is_surfaced_in_the_overworld() {
        let noises = SurfaceNoises::with_biomes(3, &[]);
        let b = biome(Builder::Overworld(PLAINS));
        let mut col = VecColumn::terrain(90, None);
        // A noise cave from 20 to 40.
        for y in 20..=40 {
            col.set(y, bb::AIR);
        }
        build_column(
            &mut col,
            &input(&b, 0, 0),
            &noises,
            &mut BedrockRandom::new(2),
        );
        assert_eq!(col.get(90), bb::GRASS_BLOCK);
        assert_eq!(col.get(19), bb::STONE, "cave floor keeps its stone");
    }
}
