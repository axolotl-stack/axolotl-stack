//! Block data generation from blocks.json.
//!
//! Generates ZST block types implementing BlockDef trait.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use crate::data_generator::block_states::{derive_state_name, to_pascal_case, to_snake_case};
use tracing::debug;

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

/// Generate blocks.rs with ZST block types implementing BlockDef.
pub fn generate_blocks(
    json_path: &Path,
    block_states_path: &Path,
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

    debug!(
        count = blocks.len(),
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

    // Generate ZST for each block
    for block in &blocks {
        let struct_name = to_pascal_case(&block.name);
        let display_name = block.display_name.replace('"', "\\\"");

        // Get state type from property analysis
        let prop_names: Vec<String> = block_props
            .get(&block.name)
            .map(|ps| ps.iter().cloned().collect())
            .unwrap_or_default();
        let state_type = derive_state_type(&prop_names);

        writeln!(out, "/// {}", block.display_name)?;
        writeln!(out, "pub struct {};", struct_name)?;
        writeln!(out)?;
        writeln!(out, "impl BlockDef for {} {{", struct_name)?;
        writeln!(out, "    const ID: u32 = {};", block.id)?;
        writeln!(
            out,
            "    const STRING_ID: &'static str = \"minecraft:{}\";",
            block.name
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
    writeln!(out, "/// All vanilla blocks as dynamic references.")?;
    writeln!(out, "pub static BLOCKS: &[&'static dyn BlockDefDyn] = &[")?;
    for block in &blocks {
        let struct_name = to_pascal_case(&block.name);
        writeln!(out, "    &{},", struct_name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;

    // Constants
    writeln!(out, "/// Number of vanilla blocks.")?;
    writeln!(out, "pub const BLOCK_COUNT: usize = {};", blocks.len())?;

    Ok(())
}
