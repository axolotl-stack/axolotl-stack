use super::density_function::DensityFunctionArg;
use super::surface_rule::RuleSource;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct NoiseSettings {
    pub aquifers_enabled: bool,
    pub ore_veins_enabled: bool,
    pub disable_mob_generation: bool,
    pub sea_level: i32,
    pub legacy_random_source: bool,
    pub noise: NoiseShape,
    pub default_block: BlockStateJson,
    pub default_fluid: BlockStateJson,
    pub noise_router: NoiseRouter,
    /// Surface rules parsed from JSON.
    pub surface_rule: RuleSource,
    /// Climate points the world spawn search aims for.
    pub spawn_target: Vec<SpawnTargetPoint>,
}

/// One `spawn_target` climate parameter point.
#[derive(Debug, Clone, Deserialize)]
pub struct SpawnTargetPoint {
    pub temperature: ClimateParameter,
    pub humidity: ClimateParameter,
    pub continentalness: ClimateParameter,
    pub erosion: ClimateParameter,
    pub depth: ClimateParameter,
    pub weirdness: ClimateParameter,
    pub offset: serde_json::Number,
}

/// A climate parameter: a single value or a `[min, max]` range. Kept as JSON numbers so
/// quantization parses the decimal text as `f32` the way Java's codec reads it.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ClimateParameter {
    Point(serde_json::Number),
    Span([serde_json::Number; 2]),
}

/// Vertical extent and cell size of the noise volume.
#[derive(Debug, Clone, Deserialize)]
pub struct NoiseShape {
    pub min_y: i32,
    pub height: i32,
    pub size_horizontal: i32,
    pub size_vertical: i32,
}

/// Build/storage geometry from `dimension_type`, not noise generation geometry or
/// the dimension type's separate portal `logical_height`.
#[derive(Debug, Clone, Deserialize)]
pub struct DimensionType {
    pub min_y: i32,
    pub height: i32,
    pub has_skylight: bool,
}

/// Resolve the normal world preset's noise-settings -> dimension-type links at
/// build time, retaining the pinned dimension JSON as the authority for storage.
pub fn parse_dimension_types(
    preset_path: &Path,
    dimension_dir: &Path,
) -> Result<HashMap<String, DimensionType>, Box<dyn std::error::Error>> {
    #[derive(Deserialize)]
    struct Preset {
        dimensions: HashMap<String, Dimension>,
    }
    #[derive(Deserialize)]
    struct Dimension {
        #[serde(rename = "type")]
        dimension_type: String,
        generator: NoiseGenerator,
    }
    #[derive(Deserialize)]
    struct NoiseGenerator {
        settings: String,
    }
    let preset: Preset = serde_json::from_str(&std::fs::read_to_string(preset_path)?)?;
    let mut dimensions = HashMap::new();
    for dimension in preset.dimensions.into_values() {
        let name = dimension
            .dimension_type
            .strip_prefix("minecraft:")
            .ok_or("non-vanilla dimension type in normal preset")?;
        let path = dimension_dir.join(format!("{name}.json"));
        let parsed: DimensionType = serde_json::from_str(&std::fs::read_to_string(&path)?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if parsed.height <= 0 || parsed.min_y % 16 != 0 || parsed.height % 16 != 0 {
            return Err(format!("{}: invalid dimension build geometry", path.display()).into());
        }
        if dimensions.insert(dimension.generator.settings, parsed).is_some() {
            return Err("normal preset reuses noise settings across dimension types".into());
        }
    }
    Ok(dimensions)
}

/// A block state as written in worldgen JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockStateJson {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Properties", default)]
    pub properties: std::collections::BTreeMap<String, String>,
}

/// Noise router containing all density function fields.
/// preliminary_surface_level is parsed as a find_top_surface density function.
#[derive(Debug, Clone, Deserialize)]
pub struct NoiseRouter {
    pub barrier: DensityFunctionArg,
    pub continents: DensityFunctionArg,
    pub depth: DensityFunctionArg,
    pub erosion: DensityFunctionArg,
    pub final_density: DensityFunctionArg,
    pub fluid_level_floodedness: DensityFunctionArg,
    pub fluid_level_spread: DensityFunctionArg,
    pub lava: DensityFunctionArg,
    /// This is a find_top_surface density function that finds where density crosses zero.
    /// It contains the inner density function (initial_density_without_jaggedness).
    pub preliminary_surface_level: DensityFunctionArg,
    pub ridges: DensityFunctionArg,
    pub temperature: DensityFunctionArg,
    pub vegetation: DensityFunctionArg,
    pub vein_gap: DensityFunctionArg,
    pub vein_ridged: DensityFunctionArg,
    pub vein_toggle: DensityFunctionArg,
}

pub fn parse_all(dir: &Path) -> Result<HashMap<String, NoiseSettings>, Box<dyn std::error::Error>> {
    let mut settings = HashMap::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let name = entry.path().file_stem().unwrap().to_string_lossy();
            let content = std::fs::read_to_string(entry.path())?;
            let parsed: NoiseSettings = serde_json::from_str(&content)
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            settings.insert(format!("minecraft:{}", name), parsed);
        }
    }

    Ok(settings)
}
