//! Entity data generation from entities.json.

use serde::Deserialize;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use tracing::debug;

/// Raw entity entry from minecraft-data entities.json.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntityJson {
    id: u32,
    #[serde(default)]
    internal_id: Option<u32>,
    name: String,
    display_name: String,
    #[serde(default)]
    height: Option<f32>,
    #[serde(default)]
    width: Option<f32>,
    #[serde(default)]
    length: Option<f32>,
    #[serde(default)]
    offset: Option<f32>,
    #[serde(default, rename = "type")]
    entity_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

/// Convert snake_case to SCREAMING_SNAKE_CASE.
fn to_screaming_snake_case(name: &str) -> String {
    name.to_uppercase()
}

/// Parse entity type string to EntityType variant.
fn parse_entity_type(s: &str) -> &'static str {
    match s {
        "animal" => "EntityType::Animal",
        "hostile" => "EntityType::Hostile",
        "passive" => "EntityType::Passive",
        "ambient" => "EntityType::Ambient",
        "mob" => "EntityType::Mob",
        "player" => "EntityType::Player",
        "living" => "EntityType::Living",
        "projectile" => "EntityType::Projectile",
        "other" => "EntityType::Other",
        _ => "EntityType::Unknown",
    }
}

/// Generate entities.rs from entities.json.
pub fn generate_entities(
    json_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(json_path)?;
    let entities: Vec<EntityJson> = serde_json::from_reader(BufReader::new(file))?;

    debug!(count = entities.len(), "Generating entity data");

    let output_path = output_dir.join("entities.rs");
    let mut out = File::create(&output_path)?;

    // Header
    writeln!(out, "//! Generated vanilla entity data.")?;
    writeln!(out, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub use valentine_bedrock_core::entity::{{EntityData, EntityType}};"
    )?;
    writeln!(out)?;

    // Track used names to avoid duplicates
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut const_names: Vec<String> = Vec::new();

    // Generate named constants
    for entity in &entities {
        let const_name = to_screaming_snake_case(&entity.name);

        // Handle duplicates by appending ID
        let final_name = if used_names.contains(&const_name) {
            format!("{}_{}", const_name, entity.id)
        } else {
            const_name.clone()
        };
        used_names.insert(final_name.clone());
        const_names.push(final_name.clone());

        let display_name = entity.display_name.replace('"', "\\\"");
        let category = entity.category.as_deref().unwrap_or("unknown");
        let entity_type_str = entity.entity_type.as_deref().unwrap_or("");
        let entity_type = parse_entity_type(entity_type_str);

        // Format Option fields
        let width = match entity.width {
            Some(w) => format!("Some({:?}_f32)", w),
            None => "None".to_string(),
        };
        let length = match entity.length {
            Some(l) => format!("Some({:?}_f32)", l),
            None => "None".to_string(),
        };
        let offset = match entity.offset {
            Some(o) => format!("Some({:?}_f32)", o),
            None => "None".to_string(),
        };

        writeln!(out, "/// {}", display_name)?;
        writeln!(out, "pub const {}: EntityData = EntityData {{", final_name)?;
        writeln!(out, "    id: {},", entity.id)?;
        writeln!(
            out,
            "    internal_id: {},",
            entity.internal_id.unwrap_or(entity.id)
        )?;
        writeln!(out, "    string_id: \"minecraft:{}\",", entity.name)?;
        writeln!(out, "    name: \"{}\",", display_name)?;
        writeln!(out, "    height: {:?}_f32,", entity.height.unwrap_or(0.0))?;
        writeln!(out, "    width: {},", width)?;
        writeln!(out, "    length: {},", length)?;
        writeln!(out, "    offset: {},", offset)?;
        writeln!(out, "    entity_type: {},", entity_type)?;
        writeln!(out, "    category: \"{}\",", category)?;
        writeln!(out, "}};")?;
        writeln!(out)?;
    }

    // Generate const array
    writeln!(out, "/// All vanilla entities for this version.")?;
    writeln!(
        out,
        "pub const ALL_ENTITIES: [EntityData; {}] = [",
        entities.len()
    )?;
    for name in &const_names {
        writeln!(out, "    {},", name)?;
    }
    writeln!(out, "];")?;

    Ok(())
}
