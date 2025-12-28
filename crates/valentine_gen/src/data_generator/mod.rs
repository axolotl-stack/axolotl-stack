//! Data generation from minecraft-data JSON files.
//!
//! This module generates static Rust data arrays from:
//! - `items.json` → Item definitions
//! - `blocks.json` → Block definitions  
//! - `blockStates.json` → Block state definitions with typed properties
//! - `entities.json` → Entity definitions
//! - `biomes.json` → Biome definitions

mod biomes;
mod block_states;
mod blocks;
mod entities;
mod items;

pub use biomes::generate_biomes;
pub use block_states::generate_block_states;
pub use blocks::generate_blocks;
pub use entities::generate_entities;
pub use items::generate_items;

use std::path::Path;

/// Configuration for which data types to generate.
#[derive(Debug, Clone, Default)]
pub struct GenerateConfig {
    pub items: bool,
    pub blocks: bool,
    pub block_states: bool,
    pub entities: bool,
    pub biomes: bool,
}

impl GenerateConfig {
    /// Generate all data types.
    #[allow(dead_code)]
    pub fn all() -> Self {
        Self {
            items: true,
            blocks: true,
            block_states: true,
            entities: true,
            biomes: true,
        }
    }

    /// Returns true if any data generation is enabled.
    pub fn any(&self) -> bool {
        self.items || self.blocks || self.block_states || self.entities || self.biomes
    }
}

/// Paths to minecraft-data JSON files for a specific version.
#[derive(Debug)]
pub struct DataPaths {
    pub items: Option<std::path::PathBuf>,
    pub blocks: Option<std::path::PathBuf>,
    pub block_states: Option<std::path::PathBuf>,
    pub entities: Option<std::path::PathBuf>,
    pub biomes: Option<std::path::PathBuf>,
}

/// Generate data modules for a specific version.
pub fn generate_version_data(
    config: &GenerateConfig,
    paths: &DataPaths,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use tracing::info;

    fs::create_dir_all(output_dir)?;

    let mut generated = Vec::new();

    if config.items {
        if let Some(ref path) = paths.items {
            if path.exists() {
                generate_items(path, output_dir)?;
                generated.push("items");
            }
        }
    }

    if config.blocks {
        if let Some(ref path) = paths.blocks {
            if path.exists() {
                // blocks.rs needs blockStates.json for property-based state type derivation
                let block_states_path = paths.block_states.as_ref();
                if let Some(states_path) = block_states_path {
                    if states_path.exists() {
                        generate_blocks(path, states_path, output_dir)?;
                        generated.push("blocks");
                    }
                } else {
                    // No blockStates available, generate without state types
                    generate_blocks(path, path, output_dir)?;
                    generated.push("blocks");
                }
            }
        }
    }

    if config.block_states {
        if let Some(ref path) = paths.block_states {
            if path.exists() {
                generate_block_states(path, output_dir)?;
                generated.push("block_states");
            }
        }
    }

    if config.entities {
        if let Some(ref path) = paths.entities {
            if path.exists() {
                generate_entities(path, output_dir)?;
                generated.push("entities");
            }
        }
    }

    if config.biomes {
        if let Some(ref path) = paths.biomes {
            if path.exists() {
                generate_biomes(path, output_dir)?;
                generated.push("biomes");
            }
        }
    }

    if !generated.is_empty() {
        info!(modules = ?generated, "Generated data modules");
    }

    Ok(())
}
