//! Bedrock surface builders and the per-biome data that parameterises them
//! (`minecraft:surface_builder`, `minecraft:surface_material_adjustments`,
//! `minecraft:mountain_parameters`), compiled into `generated::BIOMES`.
//!
//! [`build_column`] turns one shaped terrain column (stone foundation,
//! aquifer water/lava, air) into its surface: it runs the biome's surface
//! builder on the surface layer, then its subsurface builder below it.
//! [`build_range`] applies a (cave) biome's subsurface builder to a y range.
//!
//! Builders (vanilla Bedrock behaviour, which follows the classic surface
//! builders of both editions):
//! - `overworld`: top / mid / sea floor layers (also the nether forests);
//! - `frozen_ocean`: overworld plus packed ice icebergs with snow caps;
//! - `swamp`: overworld plus water puddles at sea level;
//! - `mesa`: badlands terracotta bands, eroded pillars, wooded plateaus;
//! - `capped`: nether floor / ceiling materials over a lava sea;
//! - `the_end`: end stone;
//! - `noise_gradient`: blocks chosen by 3D noise ranges (sulfur caves).

/// Materials of the overworld-family builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Materials {
    pub top: u32,
    pub mid: u32,
    pub sea_floor: u32,
    pub foundation: u32,
    pub sea: u32,
    pub sea_floor_depth: i32,
}

/// `minecraft:noise_gradient` parameters (sulfur caves).
#[derive(Debug, Clone, Copy)]
pub struct NoiseGradient {
    pub noise_name: &'static str,
    pub first_octave: i32,
    pub amplitudes: &'static [f32],
    pub non_replaceable: fn(u32) -> bool,
    /// `(min, max, block)` noise ranges.
    pub specifiers: &'static [(f32, f32, u32)],
}

/// A surface builder type with its parameters.
#[derive(Debug, Clone, Copy)]
pub enum Builder {
    Overworld(Materials),
    FrozenOcean(Materials),
    Swamp {
        materials: Materials,
        max_puddle_depth_below_sea_level: i32,
    },
    Mesa {
        materials: Materials,
        clay: u32,
        hard_clay: u32,
        bryce_pillars: bool,
        has_forest: bool,
    },
    Capped {
        floor: &'static [u32],
        ceiling: &'static [u32],
        sea: u32,
        foundation: u32,
        beach: Option<u32>,
    },
    TheEnd,
    NoiseGradient(NoiseGradient),
}

/// One `surface_material_adjustments` entry: materials that replace the
/// builder's where the adjustment noise falls in `noise_range`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjustment {
    pub top: Option<u32>,
    pub mid: Option<u32>,
    pub sea_floor: Option<u32>,
    pub foundation: Option<u32>,
    pub sea: Option<u32>,
    pub noise_frequency_scale: f32,
    pub noise_range: (f32, f32),
    /// Inclusive y range the adjustment applies to.
    pub height_range: Option<(i32, i32)>,
}

/// `minecraft:mountain_parameters`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mountain {
    /// Material and the slopes (north, south, west, east) that get it.
    pub steep_material: Option<(u32, [bool; 4])>,
    pub top_slide_enabled: Option<bool>,
}

/// Generation data of one biome.
#[derive(Debug, Clone, Copy)]
pub struct BiomeData {
    pub id: u16,
    pub identifier: &'static str,
    pub temperature: f32,
    pub downfall: f32,
    /// Carries the `frozen` tag.
    pub frozen: bool,
    /// Part of overworld generation.
    pub overworld: bool,
    pub surface: Option<Builder>,
    pub subsurface: Option<Builder>,
    pub adjustments: &'static [Adjustment],
    pub mountain: Option<Mountain>,
}

mod capped;
mod frozen_ocean;
mod mesa;
mod noise_gradient;
mod noises;
mod overworld;
mod swamp;
mod the_end;

pub use noises::SurfaceNoises;

use super::generated::bb;
use super::rng::BedrockRandom;

/// Coordinate scale of the surface noise that sets the layer depth (vanilla
/// 1/16, also Mojang's documented default `noise_frequency_scale`).
const DEPTH_NOISE_SCALE: f64 = 0.0625;
/// Classic builders worked on the surface noise times this factor; their
/// thresholds divided by it are exactly the Bedrock adjustment ranges (e.g.
/// mega taiga coarse dirt above 1.75 → 0.212), so the shared noise is
/// rescaled by it wherever a classic formula is used.
const LEGACY_NOISE_SCALE: f64 = 8.25;
/// A slope is steep when the neighbour uphill is at least this much higher
/// than the neighbour downhill (vanilla: 4 blocks across the 2-block span
/// between the two neighbours of the column).
const STEEP_HEIGHT_DIFFERENCE: i32 = 4;
/// Water open to the sky freezes below this temperature (vanilla).
const FREEZE_TEMPERATURE: f32 = 0.15;

/// One block column of a chunk after terrain shaping: stone (deepslate deep
/// down) as foundation, water/lava from aquifers and the sea, air.
pub trait SurfaceColumn {
    /// Lowest y (inclusive).
    fn min_y(&self) -> i32;
    /// Highest y (exclusive).
    fn max_y(&self) -> i32;
    /// Runtime id at `y` (`min_y <= y < max_y`).
    fn get(&self, y: i32) -> u32;
    /// Set `y` (`min_y <= y < max_y`).
    fn set(&mut self, y: i32, id: u32);
}

/// What [`build_column`] needs to know about a column besides its blocks.
#[derive(Debug, Clone, Copy)]
pub struct ColumnInput<'a> {
    /// World x of the column.
    pub x: i32,
    /// World z of the column.
    pub z: i32,
    /// Biome of the column's surface.
    pub biome: &'a BiomeData,
    /// Dimension sea level (63 overworld, 32 nether): water fills up to
    /// `sea_level - 1`.
    pub sea_level: i32,
    /// y of the highest non-air, non-liquid block of the four horizontal
    /// neighbour columns: north (z - 1), south (z + 1), west (x - 1), east
    /// (x + 1). Used for `mountain_parameters` steep slopes.
    pub neighbour_heights: [i32; 4],
}

/// Build the surface of one column: the biome's surface builder on the
/// surface layer, freezing of the top water in frozen biomes, then the
/// subsurface builder (if any) on everything below the surface layer.
pub fn build_column<C: SurfaceColumn>(
    col: &mut C,
    input: &ColumnInput<'_>,
    noises: &SurfaceNoises,
    rng: &mut BedrockRandom,
) {
    let biome = input.biome;
    let mut c = Column::new(input, noises, rng, col.min_y(), col.max_y(), true);
    let mut bottom = c.hi;
    if let Some(builder) = &biome.surface
        && let Some(y) = run(col, builder, &c, rng)
    {
        bottom = y;
    }
    if biome.frozen && c.temperature < FREEZE_TEMPERATURE {
        freeze_top_water(col);
    }
    if let Some(builder) = &biome.subsurface {
        c.hi = bottom;
        c.surface = false;
        c.topmost_only = false;
        c.steep = None;
        if c.lo < c.hi {
            run(col, builder, &c, rng);
        }
    }
}

/// Apply `input.biome`'s subsurface builder to `min_y..max_y` of the column
/// (a cave biome's part of the column). Does nothing for biomes without
/// one: cave biomes with only a surface builder (lush caves, ...) keep their
/// stone, their floors are decorated by features.
pub fn build_range<C: SurfaceColumn>(
    col: &mut C,
    input: &ColumnInput<'_>,
    noises: &SurfaceNoises,
    rng: &mut BedrockRandom,
    min_y: i32,
    max_y: i32,
) {
    let Some(builder) = &input.biome.subsurface else {
        return;
    };
    let (lo, hi) = (min_y.max(col.min_y()), max_y.min(col.max_y()));
    if lo >= hi {
        return;
    }
    let c = Column::new(input, noises, rng, lo, hi, false);
    run(col, builder, &c, rng);
}

/// Temperature the surface pass uses at a column: the biome temperature,
/// with vanilla's warm open-water patches (0.2) in frozen-ocean-builder
/// biomes.
pub fn local_temperature(noises: &SurfaceNoises, biome: &BiomeData, x: i32, z: i32) -> f32 {
    match biome.surface {
        Some(Builder::FrozenOcean(_)) => noises.frozen_temperature(x, z, biome.temperature),
        _ => biome.temperature,
    }
}

/// Whether the surface pass freezes the top water of a column.
pub fn freezes(noises: &SurfaceNoises, biome: &BiomeData, x: i32, z: i32) -> bool {
    biome.frozen && local_temperature(noises, biome, x, z) < FREEZE_TEMPERATURE
}

/// Per-column state shared by the builders.
struct Column<'a> {
    x: i32,
    z: i32,
    sea_level: i32,
    biome: &'a BiomeData,
    noises: &'a SurfaceNoises,
    /// Surface noise at the default scale, roughly `[-1, 1]`.
    noise: f64,
    /// Mid layer depth below the top block (may be `<= 0`).
    depth: i32,
    /// y window the builder may change (`lo` inclusive, `hi` exclusive).
    lo: i32,
    hi: i32,
    /// Surface pass (`false`: subsurface / cave range).
    surface: bool,
    /// Only the first solid run from the top is surfaced. Set for overworld
    /// biomes: their noise caves keep bare stone floors in BDS, the surface
    /// builder only dresses the terrain surface. Nether biomes surface every
    /// floor (the classic builders did, and the nether is all cave).
    topmost_only: bool,
    /// Steep material replacing top/mid on this column's surface.
    steep: Option<u32>,
    temperature: f32,
}

impl<'a> Column<'a> {
    fn new(
        input: &ColumnInput<'a>,
        noises: &'a SurfaceNoises,
        rng: &mut BedrockRandom,
        lo: i32,
        hi: i32,
        surface: bool,
    ) -> Self {
        let biome = input.biome;
        let noise = noises.surface(input.x, input.z, DEPTH_NOISE_SCALE);
        let steep = if surface {
            biome
                .mountain
                .and_then(|m| m.steep_material)
                .and_then(|(block, slopes)| {
                    is_steep(slopes, input.neighbour_heights).then_some(block)
                })
        } else {
            None
        };
        Self {
            x: input.x,
            z: input.z,
            sea_level: input.sea_level,
            biome,
            noises,
            noise,
            depth: surface_depth(noise, rng),
            lo,
            hi,
            surface,
            topmost_only: surface && biome.overworld,
            steep,
            temperature: local_temperature(noises, biome, input.x, input.z),
        }
    }

    /// Whether adjustment `a` applies to a surface block at `y`: inside the
    /// optional height range and the surface noise, sampled at the
    /// adjustment's frequency scale, inside `noise_range` (inclusive).
    fn adjustment_applies(&self, a: &Adjustment, y: i32) -> bool {
        if let Some((lo, hi)) = a.height_range
            && (y < lo || y > hi)
        {
            return false;
        }
        let n = self
            .noises
            .surface(self.x, self.z, a.noise_frequency_scale as f64);
        n >= a.noise_range.0 as f64 && n <= a.noise_range.1 as f64
    }

    /// `base` with the biome's adjustments for a surface block at `y`.
    /// Every matching adjustment applies, in listed order (Mojang's biome
    /// documentation; e.g. frozen peaks overrides mid and top with two
    /// overlapping entries).
    fn materials(&self, base: &Materials, y: i32) -> Materials {
        let mut m = *base;
        for a in self.biome.adjustments {
            if !self.adjustment_applies(a, y) {
                continue;
            }
            if let Some(b) = a.top {
                m.top = b;
            }
            if let Some(b) = a.mid {
                m.mid = b;
            }
            if let Some(b) = a.sea_floor {
                m.sea_floor = b;
            }
            if let Some(b) = a.foundation {
                m.foundation = b;
            }
            if let Some(b) = a.sea {
                m.sea = b;
            }
        }
        m
    }
}

fn run<C: SurfaceColumn>(
    col: &mut C,
    builder: &Builder,
    c: &Column<'_>,
    rng: &mut BedrockRandom,
) -> Option<i32> {
    match *builder {
        Builder::Overworld(m) => overworld::build(col, c, &m, rng),
        Builder::FrozenOcean(m) => frozen_ocean::build(col, c, &m, rng),
        Builder::Swamp {
            materials,
            max_puddle_depth_below_sea_level,
        } => swamp::build(col, c, &materials, max_puddle_depth_below_sea_level, rng),
        Builder::Mesa {
            materials,
            clay,
            hard_clay,
            bryce_pillars,
            has_forest,
        } => mesa::build(
            col,
            c,
            &mesa::Mesa {
                materials,
                clay,
                hard_clay,
                bryce_pillars,
                has_forest,
            },
        ),
        Builder::Capped {
            floor,
            ceiling,
            sea,
            foundation,
            beach,
        } => capped::build(
            col,
            c,
            &capped::Capped {
                floor,
                ceiling,
                sea,
                foundation,
                beach,
            },
            rng,
        ),
        Builder::TheEnd => the_end::build(col, c),
        Builder::NoiseGradient(g) => noise_gradient::build(col, c, &g),
    }
}

/// Blocks of mid material below the top block: vanilla
/// `noise * 2.75 + 3 + U[0, 0.25)`, truncated toward zero (the classic
/// `legacy_noise / 3 + 3`). One draw per column.
fn surface_depth(noise: f64, rng: &mut BedrockRandom) -> i32 {
    (noise * 2.75 + 3.0 + rng.next_float() as f64 * 0.25) as i32
}

/// Whether a flagged slope (north, south, west, east) is steep. A slope
/// facing north descends toward the north, so its south neighbour is the
/// higher one; the other directions likewise.
fn is_steep(slopes: [bool; 4], [north, south, west, east]: [i32; 4]) -> bool {
    let d = STEEP_HEIGHT_DIFFERENCE;
    (slopes[0] && south - north >= d)
        || (slopes[1] && north - south >= d)
        || (slopes[2] && east - west >= d)
        || (slopes[3] && west - east >= d)
}

#[inline]
fn is_liquid(id: u32) -> bool {
    bb::water(id) || bb::lava(id)
}

/// Terrain foundation a builder may dress: the terrain's stone, or the
/// builder's own foundation material (nether terrain may already be
/// netherrack). Other blocks (deepslate, bedrock, ice) are left alone.
#[inline]
fn is_foundation(id: u32, foundation: u32) -> bool {
    id == bb::STONE || id == foundation
}

/// A surface block at `y` is underwater when it is below the water surface
/// level or covered by liquid (also lakes above sea level).
#[inline]
fn underwater<C: SurfaceColumn>(col: &C, y: i32, sea_level: i32) -> bool {
    y < sea_level - 1 || (y + 1 < col.max_y() && is_liquid(col.get(y + 1)))
}

/// Highest non-air block of `lo..hi` and its id.
fn first_non_air<C: SurfaceColumn>(col: &C, lo: i32, hi: i32) -> Option<(i32, u32)> {
    (lo..hi)
        .rev()
        .map(|y| (y, col.get(y)))
        .find(|&(_, b)| !bb::air(b))
}

/// `sea_material` fills the open sea: in overworld biomes the air between
/// the first non-air block from the top and sea level (surface pass only);
/// in other dimensions every air block of the window below sea level, as
/// the classic nether generator floods its caverns with lava.
///
/// Return an exclusive upper bound on occupied blocks after filling. Overworld
/// surface builders can reuse the discovered top instead of scanning empty sky
/// again; subsurface and other-dimension windows remain unchanged.
fn fill_sea<C: SurfaceColumn>(col: &mut C, c: &Column<'_>, sea: u32) -> i32 {
    let top = c.hi.min(c.sea_level);
    if !c.biome.overworld {
        for y in c.lo..top {
            if bb::air(col.get(y)) {
                col.set(y, sea);
            }
        }
    } else if c.surface {
        let floor = first_non_air(col, c.lo, c.hi).map_or(c.lo, |(y, _)| y + 1);
        for y in floor..top {
            col.set(y, sea);
        }
        return floor.max(top);
    }
    c.hi
}

/// Replace the terrain stone of `lo..hi` with `foundation`.
fn convert_foundation<C: SurfaceColumn>(col: &mut C, lo: i32, hi: i32, foundation: u32) {
    if foundation == bb::STONE {
        return;
    }
    for y in lo..hi {
        if col.get(y) == bb::STONE {
            col.set(y, foundation);
        }
    }
}

/// Freeze the column's top block to ice when it is water open to the sky.
fn freeze_top_water<C: SurfaceColumn>(col: &mut C) {
    if let Some((y, b)) = first_non_air(col, col.min_y(), col.max_y())
        && bb::water(b)
    {
        col.set(y, bb::ICE);
    }
}

#[cfg(test)]
mod test_support {
    use super::{BiomeData, Builder, ColumnInput, Materials, SurfaceColumn, bb};

    pub(super) const MIN_Y: i32 = -64;
    pub(super) const MAX_Y: i32 = 320;
    pub(super) const SEA: i32 = 63;

    pub(super) const PLAINS: Materials = Materials {
        top: bb::GRASS_BLOCK,
        mid: bb::DIRT,
        sea_floor: bb::GRAVEL,
        foundation: bb::STONE,
        sea: bb::WATER,
        sea_floor_depth: 7,
    };

    /// A `Vec`-backed column.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct VecColumn {
        pub(super) min_y: i32,
        pub(super) blocks: Vec<u32>,
    }

    impl VecColumn {
        /// Stone up to `top` (inclusive), water from `top + 1` to
        /// `water_top` (inclusive) if given, air above.
        pub(super) fn terrain(top: i32, water_top: Option<i32>) -> Self {
            let blocks = (MIN_Y..MAX_Y)
                .map(|y| {
                    if y <= top {
                        bb::STONE
                    } else if water_top.is_some_and(|w| y <= w) {
                        bb::WATER
                    } else {
                        bb::AIR
                    }
                })
                .collect();
            Self {
                min_y: MIN_Y,
                blocks,
            }
        }

        /// Number of blocks of the column satisfying `pred`.
        pub(super) fn count(&self, pred: impl Fn(u32) -> bool) -> usize {
            self.blocks.iter().filter(|&&b| pred(b)).count()
        }
    }

    impl SurfaceColumn for VecColumn {
        fn min_y(&self) -> i32 {
            self.min_y
        }
        fn max_y(&self) -> i32 {
            self.min_y + self.blocks.len() as i32
        }
        fn get(&self, y: i32) -> u32 {
            self.blocks[(y - self.min_y) as usize]
        }
        fn set(&mut self, y: i32, id: u32) {
            self.blocks[(y - self.min_y) as usize] = id;
        }
    }

    /// A temperate overworld biome with the given surface builder.
    pub(super) fn biome(surface: Builder) -> BiomeData {
        BiomeData {
            id: 0,
            identifier: "test:biome",
            temperature: 0.8,
            downfall: 0.4,
            frozen: false,
            overworld: true,
            surface: Some(surface),
            subsurface: None,
            adjustments: &[],
            mountain: None,
        }
    }

    /// Flat-neighbourhood input at `(x, z)`.
    pub(super) fn input(biome: &BiomeData, x: i32, z: i32) -> ColumnInput<'_> {
        ColumnInput {
            x,
            z,
            biome,
            sea_level: SEA,
            neighbour_heights: [70; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{PLAINS, SEA, VecColumn, biome, input};
    use super::*;

    fn noises() -> SurfaceNoises {
        SurfaceNoises::with_biomes(42, &[])
    }

    #[test]
    fn overworld_puts_top_mid_foundation_in_order() {
        let noises = noises();
        let b = biome(Builder::Overworld(PLAINS));
        for x in (0..256).step_by(17) {
            let mut col = VecColumn::terrain(70, None);
            let mut rng = BedrockRandom::new(x as u32);
            let expected =
                surface_depth(noises.surface(x, 3, DEPTH_NOISE_SCALE), &mut rng.clone()).max(0);
            build_column(&mut col, &input(&b, x, 3), &noises, &mut rng);
            assert_eq!(col.get(70), bb::GRASS_BLOCK);
            assert_eq!(col.get(71), bb::AIR);
            let dirt = (col.min_y..70)
                .rev()
                .take_while(|&y| col.get(y) == bb::DIRT)
                .count() as i32;
            assert_eq!(dirt, expected, "x {x}");
            assert!(dirt <= 8);
            assert!((col.min_y..70 - dirt).all(|y| col.get(y) == bb::STONE));
        }
    }

    #[test]
    fn deep_floor_uses_sea_floor_material() {
        let noises = noises();
        let b = biome(Builder::Overworld(PLAINS));
        let mut col = VecColumn::terrain(30, Some(SEA - 1));
        build_column(
            &mut col,
            &input(&b, 5, 5),
            &noises,
            &mut BedrockRandom::new(1),
        );
        assert_eq!(col.get(30), bb::GRAVEL);
        assert_eq!(
            col.count(|id| id == bb::GRAVEL),
            1,
            "one sea-floor block, not a layer"
        );
        assert_eq!(col.get(29), bb::STONE);
        assert_eq!(col.count(|id| id == bb::GRASS_BLOCK || id == bb::DIRT), 0);
        assert_eq!(col.count(|id| id == bb::WATER), (SEA - 31) as usize);
    }

    #[test]
    fn shallow_floor_uses_mid_material() {
        let noises = noises();
        let b = biome(Builder::Overworld(PLAINS));
        let mut col = VecColumn::terrain(SEA - 3, Some(SEA - 1));
        build_column(
            &mut col,
            &input(&b, 5, 5),
            &noises,
            &mut BedrockRandom::new(1),
        );
        assert_eq!(col.get(SEA - 3), bb::DIRT);
        assert_eq!(col.count(|id| id == bb::GRASS_BLOCK || id == bb::GRAVEL), 0);
    }

    #[test]
    fn open_sea_is_filled_and_frozen_biomes_freeze() {
        let noises = noises();
        let mut b = biome(Builder::Overworld(PLAINS));
        let mut col = VecColumn::terrain(40, None);
        build_column(
            &mut col,
            &input(&b, 0, 0),
            &noises,
            &mut BedrockRandom::new(1),
        );
        assert!((41..SEA).all(|y| col.get(y) == bb::WATER));
        assert_eq!(col.get(SEA), bb::AIR);

        b.frozen = true;
        b.temperature = 0.0;
        let mut col = VecColumn::terrain(40, Some(SEA - 1));
        build_column(
            &mut col,
            &input(&b, 0, 0),
            &noises,
            &mut BedrockRandom::new(1),
        );
        assert_eq!(col.get(SEA - 1), bb::ICE);
        assert_eq!(col.get(SEA - 2), bb::WATER);
    }

    #[test]
    fn adjustments_override_by_noise_and_height() {
        const SAND_TOP: Adjustment = Adjustment {
            top: Some(bb::SAND),
            mid: None,
            sea_floor: None,
            foundation: None,
            sea: None,
            noise_frequency_scale: 0.0625,
            noise_range: (-10.0, 10.0),
            height_range: None,
        };
        static ALWAYS: [Adjustment; 1] = [SAND_TOP];
        static NEVER: [Adjustment; 1] = [Adjustment {
            noise_range: (5.0, 6.0),
            ..SAND_TOP
        }];
        static TOO_LOW: [Adjustment; 1] = [Adjustment {
            height_range: Some((0, 10)),
            ..SAND_TOP
        }];
        let noises = noises();
        for (adjustments, top) in [
            (&ALWAYS, bb::SAND),
            (&NEVER, bb::GRASS_BLOCK),
            (&TOO_LOW, bb::GRASS_BLOCK),
        ] {
            let b = BiomeData {
                adjustments,
                ..biome(Builder::Overworld(PLAINS))
            };
            let mut col = VecColumn::terrain(70, None);
            build_column(
                &mut col,
                &input(&b, 9, 9),
                &noises,
                &mut BedrockRandom::new(3),
            );
            assert_eq!(col.get(70), top);
            assert!(col.get(69) == bb::DIRT || col.get(69) == bb::STONE);
        }
    }

    #[test]
    fn later_adjustments_win() {
        static BOTH: [Adjustment; 2] = [
            Adjustment {
                top: Some(bb::SAND),
                mid: Some(bb::SAND),
                sea_floor: None,
                foundation: None,
                sea: None,
                noise_frequency_scale: 0.0625,
                noise_range: (-10.0, 10.0),
                height_range: None,
            },
            Adjustment {
                top: Some(bb::PACKED_ICE),
                mid: None,
                sea_floor: None,
                foundation: None,
                sea: None,
                noise_frequency_scale: 0.0625,
                noise_range: (-10.0, 10.0),
                height_range: None,
            },
        ];
        let noises = noises();
        let b = BiomeData {
            adjustments: &BOTH,
            ..biome(Builder::Overworld(PLAINS))
        };
        let c = Column::new(
            &input(&b, 1, 1),
            &noises,
            &mut BedrockRandom::new(0),
            -64,
            320,
            true,
        );
        let m = c.materials(&PLAINS, 70);
        assert_eq!(
            (m.top, m.mid, m.sea_floor),
            (bb::PACKED_ICE, bb::SAND, bb::GRAVEL)
        );
    }

    #[test]
    fn steep_slopes_get_steep_material() {
        let noises = noises();
        let b = BiomeData {
            mountain: Some(Mountain {
                steep_material: Some((bb::PACKED_ICE, [true, false, false, false])),
                top_slide_enabled: None,
            }),
            ..biome(Builder::Overworld(PLAINS))
        };
        // Rising toward the south: a north-facing slope.
        for (heights, top) in [
            ([60, 80, 70, 70], bb::PACKED_ICE),
            ([80, 60, 70, 70], bb::GRASS_BLOCK),
            ([68, 71, 70, 70], bb::GRASS_BLOCK),
        ] {
            let mut col = VecColumn::terrain(70, None);
            let inp = ColumnInput {
                neighbour_heights: heights,
                ..input(&b, 0, 0)
            };
            build_column(&mut col, &inp, &noises, &mut BedrockRandom::new(5));
            assert_eq!(col.get(70), top, "{heights:?}");
        }
    }

    #[test]
    fn nether_biomes_flood_caverns_and_surface_every_floor() {
        let noises = noises();
        let m = Materials {
            top: bb::GRAVEL,
            mid: bb::GRAVEL,
            sea_floor: bb::GRAVEL,
            foundation: bb::DEEPSLATE,
            sea: bb::LAVA,
            sea_floor_depth: 7,
        };
        let b = BiomeData {
            overworld: false,
            ..biome(Builder::Overworld(m))
        };
        let mut col = VecColumn::terrain(100, None);
        for y in 21..=60 {
            col.set(y, bb::AIR);
        }
        let inp = ColumnInput {
            sea_level: 32,
            ..input(&b, 0, 0)
        };
        build_column(&mut col, &inp, &noises, &mut BedrockRandom::new(5));
        assert!((21..32).all(|y| col.get(y) == bb::LAVA));
        assert!((32..=60).all(|y| col.get(y) == bb::AIR));
        assert_eq!(col.get(100), bb::GRAVEL);
        assert_eq!(
            col.get(20),
            bb::GRAVEL,
            "floor under the lava sea is surfaced too"
        );
        // Remaining terrain stone became the builder's foundation.
        assert_eq!(col.count(|id| id == bb::STONE), 0);
        assert!(col.get(-60) == bb::DEEPSLATE);
    }

    #[test]
    fn steep_needs_flag_and_difference() {
        assert!(is_steep([false, false, false, true], [70, 70, 75, 71]));
        assert!(!is_steep([false, false, true, false], [70, 70, 75, 71]));
        assert!(is_steep([false, false, true, false], [70, 70, 71, 75]));
        assert!(is_steep([false, true, false, false], [74, 70, 0, 0]));
        assert!(!is_steep([true; 4], [70, 73, 70, 73]));
    }
}
