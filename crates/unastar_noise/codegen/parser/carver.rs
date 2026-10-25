//! `configured_carver/*.json` -> IR.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

use super::surface_rule::VerticalAnchor;

/// A float provider (`FloatProvider` in the data).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FloatProvider {
    Constant(f32),
    Typed(TypedFloatProvider),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum TypedFloatProvider {
    #[serde(rename = "minecraft:constant")]
    Constant { value: f32 },
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: f32,
        max_exclusive: f32,
    },
    #[serde(rename = "minecraft:trapezoid")]
    Trapezoid { min: f32, max: f32, plateau: f32 },
}

/// A height provider (`HeightProvider` in the data).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum HeightProvider {
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: VerticalAnchor,
        max_inclusive: VerticalAnchor,
    },
}

/// Fields shared by the cave and nether cave carvers.
#[derive(Debug, Clone, Deserialize)]
pub struct CaveConfig {
    pub probability: f32,
    pub y: HeightProvider,
    #[serde(rename = "yScale")]
    pub y_scale: FloatProvider,
    pub lava_level: VerticalAnchor,
    pub replaceable: String,
    pub horizontal_radius_multiplier: FloatProvider,
    pub vertical_radius_multiplier: FloatProvider,
    pub floor_level: FloatProvider,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CanyonShape {
    pub distance_factor: FloatProvider,
    pub thickness: FloatProvider,
    pub width_smoothness: i32,
    pub horizontal_radius_factor: FloatProvider,
    pub vertical_radius_default_factor: f32,
    pub vertical_radius_center_factor: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CanyonConfig {
    pub probability: f32,
    pub y: HeightProvider,
    #[serde(rename = "yScale")]
    pub y_scale: FloatProvider,
    pub lava_level: VerticalAnchor,
    pub replaceable: String,
    pub vertical_rotation: FloatProvider,
    pub shape: CanyonShape,
}

/// A configured carver.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum ConfiguredCarver {
    #[serde(rename = "minecraft:cave")]
    Cave(CaveConfig),
    #[serde(rename = "minecraft:nether_cave")]
    NetherCave(CaveConfig),
    #[serde(rename = "minecraft:canyon")]
    Canyon(CanyonConfig),
}

/// Every configured carver keyed by namespaced name.
pub fn parse_all(dir: &Path) -> Result<BTreeMap<String, ConfiguredCarver>, String> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        if entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = format!(
            "minecraft:{}",
            entry.path().file_stem().unwrap().to_string_lossy()
        );
        let text = std::fs::read_to_string(entry.path())
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        let carver: ConfiguredCarver =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", entry.path().display()))?;
        out.insert(name, carver);
    }
    Ok(out)
}
