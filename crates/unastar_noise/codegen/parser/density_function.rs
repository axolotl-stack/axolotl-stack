use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A density function argument - can be constant, reference, or inline
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DensityFunctionArg {
    Constant(f64),
    Reference(String),
    Inline(Box<DensityFunctionDef>),
}

/// All density function types from Minecraft
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum DensityFunctionDef {
    #[serde(rename = "minecraft:constant")]
    Constant { argument: f64 },

    #[serde(rename = "minecraft:add")]
    Add {
        argument1: DensityFunctionArg,
        argument2: DensityFunctionArg,
    },

    #[serde(rename = "minecraft:mul")]
    Mul {
        argument1: DensityFunctionArg,
        argument2: DensityFunctionArg,
    },

    #[serde(rename = "minecraft:min")]
    Min {
        argument1: DensityFunctionArg,
        argument2: DensityFunctionArg,
    },

    #[serde(rename = "minecraft:max")]
    Max {
        argument1: DensityFunctionArg,
        argument2: DensityFunctionArg,
    },

    #[serde(rename = "minecraft:clamp")]
    Clamp {
        input: DensityFunctionArg,
        min: f64,
        max: f64,
    },

    #[serde(rename = "minecraft:abs")]
    Abs { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:square")]
    Square { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:cube")]
    Cube { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:squeeze")]
    Squeeze { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:half_negative")]
    HalfNegative { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:quarter_negative")]
    QuarterNegative { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:noise")]
    Noise {
        noise: String,
        xz_scale: f64,
        y_scale: f64,
    },

    #[serde(rename = "minecraft:shifted_noise")]
    ShiftedNoise {
        noise: String,
        shift_x: DensityFunctionArg,
        shift_y: DensityFunctionArg,
        shift_z: DensityFunctionArg,
        xz_scale: f64,
        y_scale: f64,
    },

    #[serde(rename = "minecraft:y_clamped_gradient")]
    YClampedGradient {
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    },

    #[serde(rename = "minecraft:flat_cache")]
    FlatCache { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:cache_2d")]
    Cache2D { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:cache_once")]
    CacheOnce { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:interpolated")]
    Interpolated { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:blend_alpha")]
    BlendAlpha {},

    #[serde(rename = "minecraft:blend_offset")]
    BlendOffset {},

    #[serde(rename = "minecraft:blend_density")]
    BlendDensity { argument: DensityFunctionArg },

    #[serde(rename = "minecraft:range_choice")]
    RangeChoice {
        input: DensityFunctionArg,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: DensityFunctionArg,
        when_out_of_range: DensityFunctionArg,
    },

    #[serde(rename = "minecraft:spline")]
    Spline { spline: SplineDef },

    #[serde(rename = "minecraft:weird_scaled_sampler")]
    WeirdScaledSampler {
        input: DensityFunctionArg,
        noise: String,
        rarity_value_mapper: String,
    },

    #[serde(rename = "minecraft:shift_a")]
    ShiftA { argument: String },

    #[serde(rename = "minecraft:shift_b")]
    ShiftB { argument: String },

    #[serde(rename = "minecraft:shift")]
    Shift { argument: String },

    #[serde(rename = "minecraft:old_blended_noise")]
    OldBlendedNoise {
        #[allow(dead_code)]
        xz_scale: f64,
        #[allow(dead_code)]
        y_scale: f64,
        #[allow(dead_code)]
        xz_factor: f64,
        #[allow(dead_code)]
        y_factor: f64,
        #[allow(dead_code)]
        smear_scale_multiplier: f64,
    },

    #[serde(rename = "minecraft:end_islands")]
    EndIslands {},

    #[serde(rename = "minecraft:invert")]
    Invert { argument: DensityFunctionArg },

    /// Find the Y level where density crosses from positive to negative.
    /// Used for preliminary_surface_level calculation.
    #[serde(rename = "minecraft:find_top_surface")]
    FindTopSurface {
        /// The density function to evaluate at each Y level
        density: DensityFunctionArg,
        /// Minimum Y to search
        lower_bound: i32,
        /// Maximum Y to search (can be a density function)
        upper_bound: DensityFunctionArg,
        /// Step size for search
        cell_height: i32,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SplineDef {
    pub coordinate: DensityFunctionArg,
    pub points: Vec<SplinePoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SplinePoint {
    pub location: f64,
    pub value: SplineValue,
    pub derivative: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SplineValue {
    Constant(f64),
    Nested(SplineDef),
}

/// Parse all density functions from a directory
pub fn parse_all(
    dir: &Path,
) -> Result<HashMap<String, DensityFunctionArg>, Box<dyn std::error::Error>> {
    let mut functions = HashMap::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let rel_path = entry.path().strip_prefix(dir)?;
            let name = rel_path
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");

            let content = std::fs::read_to_string(entry.path())?;
            match serde_json::from_str::<DensityFunctionArg>(&content) {
                Ok(def) => {
                    functions.insert(format!("minecraft:{}", name), def);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to parse {}: {}",
                        entry.path().display(),
                        e
                    );
                }
            }
        }
    }

    Ok(functions)
}
