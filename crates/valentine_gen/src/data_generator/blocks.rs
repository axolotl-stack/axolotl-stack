//! Block data generation from blocks.json + canonical_block_states.nbt.
//!
//! Generates ZST block types implementing BlockDef trait.
//! When canonical data is available (from pmmp/BedrockData), runtime IDs
//! come from the canonical list. Otherwise falls back to minecraft-data IDs.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use crate::data_generator::block_states::{derive_state_name, to_pascal_case, to_snake_case};
use crate::data_generator::canonical_blocks::{CanonicalState, generate_block_palette_blob};
use tracing::{debug, info};

/// Raw block entry from minecraft-data blocks.json.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockJson {
    id: u32,
    name: String,
    display_name: String,
    #[serde(default)]
    hardness: f32,
    #[serde(default)]
    resistance: f32,
    #[serde(default)]
    transparent: bool,
    #[serde(default)]
    emit_light: u8,
    #[serde(default)]
    filter_light: u8,
    #[serde(default)]
    min_state_id: u32,
    #[serde(default)]
    max_state_id: u32,
}

/// Block state entry from blockStates.json.
#[derive(Debug, Deserialize)]
struct BlockStateJson {
    name: String,
    #[serde(default)]
    states: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LegacyData {
    blocks: HashMap<String, String>,
}

/// Canonical block info derived from canonical_block_states.nbt.
struct CanonicalBlockInfo {
    min_state_id: u32,
    max_state_id: u32,
}

/// Derive state type for blocks.rs by using shared derive_state_name and formatting path.
fn derive_state_type(prop_names: &[String]) -> String {
    if prop_names.is_empty() {
        return "()".to_string();
    }

    if let Some(state_name) = derive_state_name(prop_names) {
        format!("super::states::{}", state_name)
    } else {
        // No pattern matched - fallback to ()
        "()".to_string()
    }
}

/// Build canonical block ID ranges from canonical state list.
/// Maps "minecraft:stone" -> CanonicalBlockInfo { min_state_id, max_state_id }.
fn build_canonical_block_map(
    canonical_states: &[CanonicalState],
) -> HashMap<String, CanonicalBlockInfo> {
    let mut map: HashMap<String, CanonicalBlockInfo> = HashMap::new();

    for state in canonical_states {
        let entry = map.entry(state.name.clone()).or_insert(CanonicalBlockInfo {
            min_state_id: state.runtime_id,
            max_state_id: state.runtime_id,
        });
        if state.runtime_id < entry.min_state_id {
            entry.min_state_id = state.runtime_id;
        }
        if state.runtime_id > entry.max_state_id {
            entry.max_state_id = state.runtime_id;
        }
    }

    map
}

/// Generate blocks.rs with ZST block types implementing BlockDef.
///
/// When `canonical_states` is provided, uses it as the authority for
/// `MIN_STATE_ID` / `MAX_STATE_ID`. Also generates block_palette.bin/rs.
pub fn generate_blocks(
    canonical_states: Option<&[CanonicalState]>,
    json_path: &Path,
    block_states_path: &Path,
    legacy_path: Option<&Path>,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(json_path)?;
    let blocks: Vec<BlockJson> = serde_json::from_reader(BufReader::new(file))?;

    // Load block states to determine property sets per block
    let states_file = File::open(block_states_path)?;
    let block_states: Vec<BlockStateJson> = serde_json::from_reader(BufReader::new(states_file))?;

    // Build map of block name -> unique property names
    let mut block_props: HashMap<String, HashSet<String>> = HashMap::new();
    for state in &block_states {
        let props = block_props.entry(state.name.clone()).or_default();
        for prop_name in state.states.keys() {
            props.insert(to_snake_case(prop_name));
        }
    }

    // Build canonical block map if available
    let canonical_map = canonical_states.map(build_canonical_block_map);

    if let Some(ref map) = canonical_map {
        info!(
            canonical_blocks = map.len(),
            minecraft_data_blocks = blocks.len(),
            "Using canonical block states for runtime IDs"
        );
    }

    // Load legacy IDs if available
    let mut legacy_ids: HashMap<String, u32> = HashMap::new();
    if let Some(path) = legacy_path {
        if path.exists() {
            debug!(path = %path.display(), "Loading legacy block IDs");
            let file = File::open(path)?;
            let legacy: LegacyData = serde_json::from_reader(BufReader::new(file))?;
            for (key, val) in legacy.blocks {
                if let Some(id_str) = key.split(':').next() {
                    if let Ok(id) = id_str.parse::<u32>() {
                        // val is "minecraft:name" or "minecraft:name[props]"
                        let name_part = val.split('[').next().unwrap_or(&val);
                        let name = name_part.strip_prefix("minecraft:").unwrap_or(name_part);
                        legacy_ids.insert(name.to_string(), id);
                    }
                }
            }
        }
    }

    // Determine the complete block list: use canonical as authority, supplemented by minecraft-data
    // We iterate canonical blocks first (if available) to get the correct ordering
    struct BlockData {
        id: u32,
        name: String,
        string_id: String,
        display_name: String,
        hardness: f32,
        resistance: f32,
        transparent: bool,
        emit_light: u8,
        filter_light: u8,
        min_state_id: u32,
        max_state_id: u32,
        prop_names: Vec<String>,
    }

    let mut block_list: Vec<BlockData> = Vec::new();

    if let Some(ref canonical_map) = canonical_map {
        // Build a lookup from minecraft-data for supplementary properties
        let mc_data_lookup: HashMap<String, &BlockJson> = blocks
            .iter()
            .map(|b| (format!("minecraft:{}", b.name), b))
            .collect();

        // Use canonical blocks as the complete list, supplemented by minecraft-data
        let mut seen = HashSet::new();
        for (canonical_name, info) in canonical_map {
            if !seen.insert(canonical_name.clone()) {
                continue;
            }

            let short_name = canonical_name
                .strip_prefix("minecraft:")
                .unwrap_or(canonical_name);

            // Get supplementary data from minecraft-data
            let (id, display_name, hardness, resistance, transparent, emit_light, filter_light) =
                if let Some(mc_block) = mc_data_lookup.get(canonical_name.as_str()) {
                    let id = if let Some(&legacy_id) = legacy_ids.get(&mc_block.name) {
                        legacy_id
                    } else {
                        mc_block.id
                    };
                    (
                        id,
                        mc_block.display_name.clone(),
                        mc_block.hardness,
                        mc_block.resistance,
                        mc_block.transparent,
                        mc_block.emit_light,
                        mc_block.filter_light,
                    )
                } else {
                    // Block exists in canonical but not in minecraft-data - use defaults
                    debug!(
                        block = %canonical_name,
                        "Block in canonical_block_states but not in minecraft-data, using defaults"
                    );
                    (
                        info.min_state_id, // Use min_state_id as fallback ID
                        to_display_name(short_name),
                        0.0,
                        0.0,
                        false,
                        0,
                        0,
                    )
                };

            let prop_names: Vec<String> = block_props
                .get(short_name)
                .map(|ps| ps.iter().cloned().collect())
                .unwrap_or_default();

            block_list.push(BlockData {
                id,
                name: short_name.to_string(),
                string_id: canonical_name.clone(),
                display_name,
                hardness,
                resistance,
                transparent,
                emit_light,
                filter_light,
                min_state_id: info.min_state_id,
                max_state_id: info.max_state_id,
                prop_names,
            });
        }

        // Sort by min_state_id for stable output (first block in canonical list first)
        block_list.sort_by_key(|b| b.min_state_id);
    } else {
        // No canonical data: fall back to minecraft-data (old behavior)
        for block in &blocks {
            let id = if let Some(&legacy_id) = legacy_ids.get(&block.name) {
                legacy_id
            } else {
                block.id
            };

            let prop_names: Vec<String> = block_props
                .get(&block.name)
                .map(|ps| ps.iter().cloned().collect())
                .unwrap_or_default();

            block_list.push(BlockData {
                id,
                name: block.name.clone(),
                string_id: format!("minecraft:{}", block.name),
                display_name: block.display_name.clone(),
                hardness: block.hardness,
                resistance: block.resistance,
                transparent: block.transparent,
                emit_light: block.emit_light,
                filter_light: block.filter_light,
                min_state_id: block.min_state_id,
                max_state_id: block.max_state_id,
                prop_names,
            });
        }
    }

    debug!(
        count = block_list.len(),
        "Generating ZST blocks with state types"
    );

    let output_path = output_dir.join("blocks.rs");
    let mut out = File::create(&output_path)?;

    // Header
    writeln!(out, "//! Generated vanilla block definitions.")?;
    writeln!(out, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(out)?;
    writeln!(
        out,
        "use valentine_bedrock_core::block::{{BlockDef, BlockDefDyn, BlockState}};"
    )?;
    writeln!(out)?;

    // Track used struct names to avoid collisions
    let mut used_names: HashSet<String> = HashSet::new();

    // Generate ZST for each block
    for block in &block_list {
        let mut struct_name = to_pascal_case(&block.name);
        // Handle name collisions
        if used_names.contains(&struct_name) {
            struct_name = format!("{}Block", struct_name);
        }
        used_names.insert(struct_name.clone());

        let display_name = block.display_name.replace('"', "\\\"");
        let state_type = derive_state_type(&block.prop_names);

        writeln!(out, "/// {}", block.display_name)?;
        writeln!(out, "pub struct {};", struct_name)?;
        writeln!(out)?;
        writeln!(out, "impl BlockDef for {} {{", struct_name)?;
        writeln!(out, "    const ID: u32 = {};", block.id)?;
        writeln!(
            out,
            "    const STRING_ID: &'static str = \"{}\";",
            block.string_id
        )?;
        writeln!(out, "    const NAME: &'static str = \"{}\";", display_name)?;
        writeln!(out, "    const HARDNESS: f32 = {:?}_f32;", block.hardness)?;
        writeln!(
            out,
            "    const RESISTANCE: f32 = {:?}_f32;",
            block.resistance
        )?;
        writeln!(
            out,
            "    const IS_TRANSPARENT: bool = {};",
            block.transparent
        )?;
        writeln!(out, "    const EMIT_LIGHT: u8 = {};", block.emit_light)?;
        writeln!(out, "    const FILTER_LIGHT: u8 = {};", block.filter_light)?;
        writeln!(out, "    const MIN_STATE_ID: u32 = {};", block.min_state_id)?;
        writeln!(out, "    const MAX_STATE_ID: u32 = {};", block.max_state_id)?;
        writeln!(out, "    type State = {};", state_type)?;
        writeln!(
            out,
            "    fn default_state() -> Self::State {{ Default::default() }}"
        )?;
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    // Generate registry array
    let mut used_names2: HashSet<String> = HashSet::new();
    writeln!(out, "/// All vanilla blocks as dynamic references.")?;
    writeln!(out, "pub static BLOCKS: &[&'static dyn BlockDefDyn] = &[")?;
    for block in &block_list {
        let mut struct_name = to_pascal_case(&block.name);
        if used_names2.contains(&struct_name) {
            struct_name = format!("{}Block", struct_name);
        }
        used_names2.insert(struct_name.clone());
        writeln!(out, "    &{},", struct_name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;

    // Constants
    writeln!(out, "/// Number of vanilla blocks.")?;
    writeln!(out, "pub const BLOCK_COUNT: usize = {};", block_list.len())?;

    // Generate block palette blob if canonical data is available
    if let Some(canonical_states) = canonical_states {
        generate_block_palette_blob(canonical_states, output_dir)?;
    }

    Ok(())
}

/// Convert a snake_case block name to a human-readable display name.
fn to_display_name(name: &str) -> String {
    name.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
