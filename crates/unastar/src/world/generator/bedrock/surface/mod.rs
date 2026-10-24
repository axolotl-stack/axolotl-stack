//! Bedrock surface builders and the per-biome data that parameterises them
//! (`minecraft:surface_builder`, `minecraft:surface_material_adjustments`,
//! `minecraft:mountain_parameters`), compiled into `generated::BIOMES`.

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
