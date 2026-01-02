//! Biome feature list code generator.
//!
//! Generates static feature list constants and lookup functions for each biome
//! and generation step.

use crate::codegen::parser::biome::{self, BiomeJson, GenerationStep};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Emit biome_features.rs with all biome feature mappings.
pub fn emit_biome_features(
    output_dir: &Path,
    biomes: &HashMap<String, BiomeJson>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut code = String::new();

    // Header
    code.push_str("// Generated biome feature mappings.\n");
    code.push_str("// Do not edit manually - regenerated at build time from worldgen JSON.\n\n");

    // Generate GenerationStep enum
    code.push_str("/// Generation steps for features.\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    code.push_str("#[repr(u8)]\n");
    code.push_str("pub enum GenerationStep {\n");
    for step in GenerationStep::ALL {
        code.push_str(&format!("    {} = {},\n", step.as_str(), step as u8));
    }
    code.push_str("}\n\n");

    // Collect all unique feature references
    let mut all_features: HashSet<String> = HashSet::new();
    for biome in biomes.values() {
        for step_features in &biome.features {
            for feature in step_features {
                all_features.insert(biome::strip_minecraft_prefix(feature).to_string());
            }
        }
    }

    // Sort biome names for deterministic output
    let mut biome_names: Vec<_> = biomes.keys().cloned().collect();
    biome_names.sort();

    // Generate feature list constants per biome per step
    for biome_name in &biome_names {
        let biome = &biomes[biome_name];
        let biome_const_prefix = biome::to_screaming_snake_case(biome_name);

        for step in GenerationStep::ALL {
            let step_idx = step as usize;
            let features = if step_idx < biome.features.len() {
                &biome.features[step_idx]
            } else {
                continue; // Skip if biome doesn't have this step
            };

            if features.is_empty() {
                continue; // Skip empty feature lists
            }

            let step_name = step.as_str().to_uppercase();
            code.push_str(&format!(
                "pub const {}_{}: &[&str] = &[\n",
                biome_const_prefix, step_name
            ));
            for feature in features {
                let feature_name = biome::strip_minecraft_prefix(feature);
                code.push_str(&format!("    \"{}\",\n", feature_name));
            }
            code.push_str("];\n\n");
        }
    }

    // Generate BiomeFeatures enum with all biome variants
    code.push_str("/// Biome identifiers for feature lookup.\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    code.push_str("pub enum BiomeFeatures {\n");
    for biome_name in &biome_names {
        let pascal_name = biome::to_pascal_case(biome_name);
        code.push_str(&format!("    {},\n", pascal_name));
    }
    code.push_str("}\n\n");

    // Generate lookup function
    code.push_str("impl BiomeFeatures {\n");
    code.push_str("    /// Get the placed feature list for a biome and generation step.\n");
    code.push_str("    /// Returns an empty slice if no features are defined for that step.\n");
    code.push_str("    pub fn get_features(&self, step: GenerationStep) -> &'static [&'static str] {\n");
    code.push_str("        match (self, step) {\n");

    for biome_name in &biome_names {
        let biome = &biomes[biome_name];
        let pascal_name = biome::to_pascal_case(biome_name);
        let const_prefix = biome::to_screaming_snake_case(biome_name);

        for step in GenerationStep::ALL {
            let step_idx = step as usize;
            let features = if step_idx < biome.features.len() {
                &biome.features[step_idx]
            } else {
                continue;
            };

            if features.is_empty() {
                continue;
            }

            let step_name = step.as_str();
            let step_upper = step_name.to_uppercase();
            code.push_str(&format!(
                "            (BiomeFeatures::{}, GenerationStep::{}) => {}_{},\n",
                pascal_name, step_name, const_prefix, step_upper
            ));
        }
    }

    code.push_str("            _ => &[],\n");
    code.push_str("        }\n");
    code.push_str("    }\n\n");

    // Generate from_name function for string lookup
    code.push_str("    /// Get biome from snake_case name (e.g., \"plains\", \"old_growth_pine_taiga\").\n");
    code.push_str("    pub fn from_name(name: &str) -> Option<Self> {\n");
    code.push_str("        match name {\n");
    for biome_name in &biome_names {
        let pascal_name = biome::to_pascal_case(biome_name);
        code.push_str(&format!(
            "            \"{}\" => Some(BiomeFeatures::{}),\n",
            biome_name, pascal_name
        ));
    }
    code.push_str("            _ => None,\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    // Generate a list of all unique placed feature names for future use
    let mut sorted_features: Vec<_> = all_features.into_iter().collect();
    sorted_features.sort();

    code.push_str("/// All unique placed feature names referenced by biomes.\n");
    code.push_str("pub const ALL_PLACED_FEATURES: &[&str] = &[\n");
    for feature in &sorted_features {
        code.push_str(&format!("    \"{}\",\n", feature));
    }
    code.push_str("];\n");

    std::fs::write(output_dir.join("biome_features.rs"), code)?;
    Ok(())
}
