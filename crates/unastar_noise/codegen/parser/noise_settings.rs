use super::density_function::DensityFunctionArg;
use super::surface_rule::{BlockState, RuleSource};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct NoiseSettings {
    pub aquifers_enabled: bool,
    pub ore_veins_enabled: bool,
    pub sea_level: i32,
    pub default_block: BlockState,
    pub default_fluid: BlockState,
    pub noise: NoiseHeightSettings,
    pub noise_router: NoiseRouter,
    /// Surface rules parsed from JSON.
    pub surface_rule: RuleSource,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NoiseHeightSettings {
    pub height: i32,
    pub min_y: i32,
    pub size_horizontal: i32,
    pub size_vertical: i32,
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
            match serde_json::from_str::<NoiseSettings>(&content) {
                Ok(ns) => {
                    settings.insert(format!("minecraft:{}", name), ns);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", entry.path().display(), e);
                }
            }
        }
    }

    Ok(settings)
}
