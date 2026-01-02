//! Biome JSON parser for feature extraction.
//!
//! Parses biome/*.json files to extract feature lists for each generation step.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Helper for deserializing fields that can be either a single string or an array.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StringOrArray {
    Single(String),
    Array(Vec<String>),
}

impl Default for StringOrArray {
    fn default() -> Self {
        StringOrArray::Array(Vec::new())
    }
}

impl StringOrArray {
    #[allow(dead_code)]
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrArray::Single(s) => vec![s],
            StringOrArray::Array(v) => v,
        }
    }
}

/// Parsed biome data from JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct BiomeJson {
    /// 11 generation steps, each with a list of placed_feature references.
    /// Index corresponds to GenerationStep::Decoration enum value.
    #[serde(default)]
    pub features: Vec<Vec<String>>,

    /// Carver references (cave, canyon, etc.) - can be a string or array in JSON.
    #[serde(default)]
    #[allow(dead_code)]
    pub carvers: StringOrArray,

    /// Temperature value for the biome (0.0 - 2.0 typical range)
    #[serde(default)]
    #[allow(dead_code)]
    pub temperature: f32,

    /// Downfall/precipitation value (0.0 - 1.0)
    #[serde(default)]
    #[allow(dead_code)]
    pub downfall: f32,

    /// Whether this biome has precipitation
    #[serde(default)]
    #[allow(dead_code)]
    pub has_precipitation: bool,
}

/// Generation steps for features.
/// Matches vanilla's GenerationStep.Decoration enum ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GenerationStep {
    RawGeneration = 0,
    Lakes = 1,
    LocalModifications = 2,
    UndergroundStructures = 3,
    SurfaceStructures = 4,
    Strongholds = 5,
    UndergroundOres = 6,
    UndergroundDecoration = 7,
    FluidSprings = 8,
    VegetalDecoration = 9,
    TopLayerModification = 10,
}

impl GenerationStep {
    pub const ALL: [GenerationStep; 11] = [
        GenerationStep::RawGeneration,
        GenerationStep::Lakes,
        GenerationStep::LocalModifications,
        GenerationStep::UndergroundStructures,
        GenerationStep::SurfaceStructures,
        GenerationStep::Strongholds,
        GenerationStep::UndergroundOres,
        GenerationStep::UndergroundDecoration,
        GenerationStep::FluidSprings,
        GenerationStep::VegetalDecoration,
        GenerationStep::TopLayerModification,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            GenerationStep::RawGeneration => "RawGeneration",
            GenerationStep::Lakes => "Lakes",
            GenerationStep::LocalModifications => "LocalModifications",
            GenerationStep::UndergroundStructures => "UndergroundStructures",
            GenerationStep::SurfaceStructures => "SurfaceStructures",
            GenerationStep::Strongholds => "Strongholds",
            GenerationStep::UndergroundOres => "UndergroundOres",
            GenerationStep::UndergroundDecoration => "UndergroundDecoration",
            GenerationStep::FluidSprings => "FluidSprings",
            GenerationStep::VegetalDecoration => "VegetalDecoration",
            GenerationStep::TopLayerModification => "TopLayerModification",
        }
    }
}

/// Parse all biome JSON files from a directory.
/// Returns a map of biome name (e.g., "plains") to parsed BiomeJson.
pub fn parse_all(dir: &Path) -> Result<HashMap<String, BiomeJson>, Box<dyn std::error::Error>> {
    let mut biomes = HashMap::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let name = entry.path().file_stem().unwrap().to_string_lossy().to_string();
            let content = std::fs::read_to_string(entry.path())?;
            let biome: BiomeJson = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse biome {}: {}", name, e))?;
            biomes.insert(name, biome);
        }
    }

    Ok(biomes)
}

/// Convert a snake_case biome name to PascalCase.
/// e.g., "old_growth_pine_taiga" -> "OldGrowthPineTaiga"
pub fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Convert a snake_case biome name to SCREAMING_SNAKE_CASE.
/// e.g., "old_growth_pine_taiga" -> "OLD_GROWTH_PINE_TAIGA"
pub fn to_screaming_snake_case(s: &str) -> String {
    s.to_uppercase()
}

/// Strip the minecraft: prefix from a feature reference.
/// e.g., "minecraft:ore_coal_upper" -> "ore_coal_upper"
pub fn strip_minecraft_prefix(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}
