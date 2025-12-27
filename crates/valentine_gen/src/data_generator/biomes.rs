//! Biome data generation from biomes.json.

use serde::Deserialize;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use tracing::debug;

/// Raw biome entry from minecraft-data biomes.json.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BiomeJson {
    id: u32,
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    temperature: f32,
    #[serde(default)]
    has_precipitation: bool,
    #[serde(default)]
    dimension: Option<String>,
    #[serde(default)]
    color: u32,
}

/// Convert snake_case to SCREAMING_SNAKE_CASE.
fn to_screaming_snake_case(name: &str) -> String {
    name.to_uppercase()
}

/// Generate biomes.rs from biomes.json.
pub fn generate_biomes(
    json_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(json_path)?;
    let biomes: Vec<BiomeJson> = serde_json::from_reader(BufReader::new(file))?;

    debug!(count = biomes.len(), "Generating biome data");

    let output_path = output_dir.join("biomes.rs");
    let mut out = File::create(&output_path)?;

    // Header
    writeln!(out, "//! Generated vanilla biome data.")?;
    writeln!(out, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(out)?;
    writeln!(out, "use valentine_bedrock_core::biome::BiomeData;")?;
    writeln!(out)?;

    // Track used names to avoid duplicates
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut const_names: Vec<String> = Vec::new();

    // Generate named constants
    for biome in &biomes {
        let const_name = to_screaming_snake_case(&biome.name);

        // Handle duplicates by appending ID
        let final_name = if used_names.contains(&const_name) {
            format!("{}_{}", const_name, biome.id)
        } else {
            const_name.clone()
        };
        used_names.insert(final_name.clone());
        const_names.push(final_name.clone());

        let display_name = biome
            .display_name
            .as_deref()
            .unwrap_or(&biome.name)
            .replace('"', "\\\"");
        let category = biome.category.as_deref().unwrap_or("unknown");
        let dimension = biome.dimension.as_deref().unwrap_or("overworld");

        writeln!(out, "/// {}", display_name)?;
        writeln!(out, "pub const {}: BiomeData = BiomeData {{", final_name)?;
        writeln!(out, "    id: {},", biome.id)?;
        writeln!(out, "    string_id: \"minecraft:{}\",", biome.name)?;
        writeln!(out, "    name: \"{}\",", display_name)?;
        writeln!(out, "    category: \"{}\",", category)?;
        writeln!(out, "    dimension: \"{}\",", dimension)?;
        writeln!(out, "    temperature: {:?}_f32,", biome.temperature)?;
        writeln!(out, "    has_precipitation: {},", biome.has_precipitation)?;
        writeln!(out, "    color: 0x{:06X},", biome.color)?;
        writeln!(out, "}};")?;
        writeln!(out)?;
    }

    // Generate const array
    writeln!(out, "/// All vanilla biomes for this version.")?;
    writeln!(
        out,
        "pub const ALL_BIOMES: [BiomeData; {}] = [",
        biomes.len()
    )?;
    for name in &const_names {
        writeln!(out, "    {},", name)?;
    }
    writeln!(out, "];")?;

    Ok(())
}
