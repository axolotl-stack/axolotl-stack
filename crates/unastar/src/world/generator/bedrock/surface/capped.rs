//! `minecraft:capped`: nether floors and ceilings (basalt deltas, soul sand
//! valley).
//!
//! Vanilla behaviour (the nether capped builder): air below sea level is
//! the lava sea; every floor (foundation with air or the sea above) gets
//! `depth` blocks of a floor material and every ceiling (foundation with air
//! or the sea below) `depth` blocks of a ceiling material, painting only
//! foundation. The material of a column is picked from the list by a noise
//! (patches of basalt / blackstone, soul sand / soul soil). With a
//! `beach_material`, floors at the sea shore (`sea_level - 4 ..= sea_level
//! + 1`) use it in noise patches (soul sand valley gravel beaches). The patch
//! noise scale and threshold are not observable from the data; they follow
//! the classic nether beaches (patch noise above `0.15 + U[0, 0.35)`),
//! statistical parity. Remaining terrain stone becomes
//! `foundation_material`.

use super::super::generated::bb;
use super::super::rng::BedrockRandom;
use super::{Column, SurfaceColumn, convert_foundation, fill_sea, is_foundation, is_liquid};

/// Beaches span this far below / above the sea level.
const BEACH_BELOW: i32 = 4;
const BEACH_ABOVE: i32 = 1;

/// Capped builder parameters.
#[derive(Debug, Clone, Copy)]
pub(super) struct Capped {
    pub(super) floor: &'static [u32],
    pub(super) ceiling: &'static [u32],
    pub(super) sea: u32,
    pub(super) foundation: u32,
    pub(super) beach: Option<u32>,
}

/// Entry of `list` for a noise value in about `[-1, 1]` (equal shares).
fn pick(list: &[u32], noise: f64) -> Option<u32> {
    if list.is_empty() {
        return None;
    }
    let t = ((noise + 1.0) * 0.5).clamp(0.0, 1.0);
    let i = ((t * list.len() as f64) as usize).min(list.len() - 1);
    Some(list[i])
}

/// Run the capped builder; returns the lowest y it dressed.
pub(super) fn build<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, p: &Capped, rng: &mut BedrockRandom) -> Option<i32> {
    // The builder has no top/mid materials; adjustments can still swap its
    // sea and foundation (applied column-wide, at sea level).
    let (mut sea, mut foundation) = (p.sea, p.foundation);
    for a in c.biome.adjustments {
        if c.adjustment_applies(a, c.sea_level) {
            sea = a.sea.unwrap_or(sea);
            foundation = a.foundation.unwrap_or(foundation);
        }
    }
    fill_sea(col, c, sea);
    let floor = pick(p.floor, c.noises.capped_floor(c.x, c.z));
    let ceiling = pick(p.ceiling, c.noises.capped_ceiling(c.x, c.z));
    let beach = p.beach.filter(|_| c.noises.beach(c.x, c.z) > 0.15 + rng.next_float() as f64 * 0.35);
    let depth = c.depth;
    let open = |b: u32| bb::air(b) || is_liquid(b);
    let mut lowest: Option<i32> = None;
    let mut note = |y: i32| lowest = Some(lowest.map_or(y, |l| l.min(y)));
    // Whether the block above the current one is open (air or sea).
    let mut above_open = c.hi >= col.max_y() || open(col.get(c.hi));
    for y in (c.lo..c.hi).rev() {
        let b = col.get(y);
        if open(b) {
            // Ceiling: the foundation run right above this opening.
            if let Some(block) = ceiling
                && !above_open
            {
                for k in 1..=depth {
                    let at = y + k;
                    if at >= c.hi || !is_foundation(col.get(at), foundation) {
                        break;
                    }
                    col.set(at, block);
                    note(at);
                }
            }
            above_open = true;
            continue;
        }
        if above_open && is_foundation(b, foundation) {
            let shore = (c.sea_level - BEACH_BELOW..=c.sea_level + BEACH_ABOVE).contains(&y);
            let material = if shore { beach.or(floor) } else { floor };
            if let Some(block) = material {
                for k in 0..depth {
                    let at = y - k;
                    if at < c.lo || !is_foundation(col.get(at), foundation) {
                        break;
                    }
                    col.set(at, block);
                    note(at);
                }
            }
        }
        above_open = false;
    }
    convert_foundation(col, c.lo, c.hi, foundation);
    lowest
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{VecColumn, biome, input};
    use super::super::{BiomeData, Builder, ColumnInput, SurfaceNoises, build_column};
    use super::*;

    static FLOOR: [u32; 2] = [bb::SAND, bb::GRAVEL];
    static CEILING: [u32; 1] = [bb::CLAY];

    fn nether() -> BiomeData {
        let builder = Builder::Capped { floor: &FLOOR, ceiling: &CEILING, sea: bb::LAVA, foundation: bb::DEEPSLATE, beach: None };
        BiomeData { overworld: false, ..biome(builder) }
    }

    #[test]
    fn floors_and_ceilings_get_their_materials() {
        let noises = SurfaceNoises::with_biomes(8, &[]);
        let b = nether();
        for x in 0..32 {
            let mut col = VecColumn::terrain(127, None);
            // Cavern 60..=80 and a lava-sea cavern 10..=40.
            for y in (60..=80).chain(10..=40) {
                col.set(y, bb::AIR);
            }
            let mut rng = BedrockRandom::new(x as u32);
            let inp = ColumnInput { sea_level: 32, ..input(&b, x * 9, 0) };
            let depth = Column::new(&inp, &noises, &mut rng.clone(), -64, 320, true).depth;
            build_column(&mut col, &inp, &noises, &mut rng);
            assert!((10..32).all(|y| col.get(y) == bb::LAVA));
            if depth <= 0 {
                continue;
            }
            for floor_top in [59, 9] {
                let f = col.get(floor_top);
                assert!(f == bb::SAND || f == bb::GRAVEL, "x {x}: floor {floor_top}");
                assert!((floor_top - depth + 1..=floor_top).all(|y| col.get(y) == f));
                assert_eq!(col.get(floor_top - depth), bb::DEEPSLATE);
            }
            for ceiling_bottom in [81, 41] {
                assert!((ceiling_bottom..ceiling_bottom + depth).all(|y| col.get(y) == bb::CLAY), "x {x}");
            }
            // The roof's top surface is a floor too (open sky above).
            let top = col.get(127);
            assert!(top == bb::SAND || top == bb::GRAVEL);
            assert_eq!(col.count(|id| id == bb::STONE), 0);
        }
    }

    #[test]
    fn pick_splits_the_noise_range() {
        let list = [1, 2, 3];
        assert_eq!(pick(&list, -1.0), Some(1));
        assert_eq!(pick(&list, 0.0), Some(2));
        assert_eq!(pick(&list, 1.0), Some(3));
        assert_eq!(pick(&list, 5.0), Some(3));
        assert_eq!(pick(&[], 0.0), None);
    }
}
