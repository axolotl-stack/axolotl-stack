//! Block state generation from blockStates.json.
//!
//! Generates:
//! 1. Shared enums (CardinalDirection, PillarAxis, etc.)
//! 2. Shared state structs (DoorState, StairState, etc.) implementing BlockState
//!
//! These are analyzed and clustered from the raw JSON data.

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use tracing::debug;

/// Raw block state entry from minecraft-data blockStates.json.
#[derive(Debug, Deserialize)]
struct BlockStateJson {
    name: String,
    #[serde(default)]
    states: HashMap<String, PropertyValue>,
}

/// Property value in a block state.
#[derive(Debug, Deserialize)]
struct PropertyValue {
    #[serde(rename = "type")]
    prop_type: String,
    value: serde_json::Value,
}

/// Aggregated property definition for a block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PropertyDef {
    pub name: String,
    pub prop_type: PropType,
    pub min: i64,
    pub max: i64,
    pub string_values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropType {
    Int,
    Byte,
    String,
}

/// Signature for clustering blocks with same state shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StateShape {
    props: Vec<PropertyDef>,
}

pub fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

pub fn to_snake_case(name: &str) -> String {
    // Remove minecraft: prefix if present
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    name.to_lowercase().replace(':', "_")
}

/// Generate block_states.rs with enums and state structs.
pub fn generate_block_states(
    json_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(json_path)?;
    let states: Vec<BlockStateJson> = serde_json::from_reader(BufReader::new(file))?;

    debug!(count = states.len(), "Parsing block states");

    // Step 1: Aggregate property definitions per block
    let mut block_props: HashMap<String, HashMap<String, PropertyDef>> = HashMap::new();

    for state in &states {
        let props = block_props.entry(state.name.clone()).or_default();

        for (prop_name, prop_val) in &state.states {
            let prop_type = match prop_val.prop_type.as_str() {
                "int" => PropType::Int,
                "byte" => PropType::Byte,
                "string" => PropType::String,
                _ => PropType::Int,
            };

            let numeric_val = match &prop_val.value {
                serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
                _ => 0,
            };

            let string_val = match &prop_val.value {
                serde_json::Value::String(s) => Some(s.clone()),
                _ => None,
            };

            let entry = props
                .entry(prop_name.clone())
                .or_insert_with(|| PropertyDef {
                    name: prop_name.clone(),
                    prop_type,
                    min: numeric_val,
                    max: numeric_val,
                    string_values: Vec::new(),
                });

            if prop_type != PropType::String {
                entry.min = entry.min.min(numeric_val);
                entry.max = entry.max.max(numeric_val);
            }

            if let Some(s) = string_val {
                if !entry.string_values.contains(&s) {
                    entry.string_values.push(s);
                }
            }
        }
    }

    debug!(blocks = block_props.len(), "Aggregated block properties");

    // Step 2: Identify unique string enums
    let mut string_enums: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for props in block_props.values() {
        for prop in props.values() {
            if prop.prop_type == PropType::String && !prop.string_values.is_empty() {
                // Use property name as enum name
                let enum_name = to_pascal_case(&to_snake_case(&prop.name));
                let existing = string_enums.entry(enum_name.clone()).or_default();
                for val in &prop.string_values {
                    if !existing.contains(val) {
                        existing.push(val.clone());
                    }
                }
            }
        }
    }

    // Step 3: Cluster blocks by state shape to identify shared state types
    let mut shape_clusters: HashMap<StateShape, Vec<String>> = HashMap::new();
    for (block_name, props) in &block_props {
        if props.is_empty() {
            continue;
        }
        let mut sorted_props: Vec<_> = props.values().cloned().collect();
        sorted_props.sort_by(|a, b| a.name.cmp(&b.name));
        let shape = StateShape {
            props: sorted_props,
        };
        shape_clusters
            .entry(shape)
            .or_default()
            .push(block_name.clone());
    }

    debug!(
        clusters = shape_clusters.len(),
        "Identified state shape clusters"
    );

    // Output file
    let output_path = output_dir.join("states.rs");
    let mut out = File::create(&output_path)?;

    writeln!(out, "//! Generated block state types.")?;
    writeln!(out, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(out)?;
    writeln!(out, "use valentine_bedrock_core::block::BlockState;")?;
    writeln!(out)?;

    // Generate enums
    writeln!(out, "// ===== SHARED ENUMS =====")?;
    writeln!(out)?;
    for (enum_name, values) in &string_enums {
        writeln!(
            out,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]"
        )?;
        writeln!(out, "#[repr(u8)]")?;
        writeln!(out, "pub enum {} {{", enum_name)?;
        for (i, val) in values.iter().enumerate() {
            let variant = to_pascal_case(val);
            if i == 0 {
                writeln!(out, "    #[default]")?;
            }
            writeln!(out, "    {} = {},", variant, i)?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;
        writeln!(out, "impl {} {{", enum_name)?;
        writeln!(out, "    pub const COUNT: u32 = {};", values.len())?;
        writeln!(out, "    pub fn from_raw(v: u8) -> Option<Self> {{")?;
        writeln!(out, "        match v {{")?;
        for (i, val) in values.iter().enumerate() {
            let variant = to_pascal_case(val);
            writeln!(out, "            {} => Some(Self::{}),", i, variant)?;
        }
        writeln!(out, "            _ => None,")?;
        writeln!(out, "        }}")?;
        writeln!(out, "    }}")?;
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    // Generate state structs for common patterns
    writeln!(out, "// ===== SHARED STATE STRUCTS =====")?;
    writeln!(out)?;

    // Find largest clusters and generate named state types for them
    let mut sorted_clusters: Vec<_> = shape_clusters.iter().collect();
    sorted_clusters.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    // Track names already used (include enum names to prevent clashes)
    let mut generated_states: HashSet<String> = string_enums.keys().cloned().collect();

    for (shape, blocks) in &sorted_clusters {
        if shape.props.is_empty() {
            continue;
        }

        // Use first block name for state struct name, or create descriptive name
        // Normalize property names for pattern matching
        let prop_names: Vec<String> = shape.props.iter().map(|p| to_snake_case(&p.name)).collect();

        let mut state_name = if let Some(name) = derive_state_name(&prop_names) {
            name
        } else if blocks.len() > 1 {
            // Try suffix detection
            if let Some(common) = find_common_suffix(blocks) {
                format!("{}State", to_pascal_case(&common))
            } else {
                // WARN: Generic State*Props fallback - add a pattern!
                tracing::warn!(
                    name = format!("State{}Props", prop_names.len()),
                    blocks = ?blocks.iter().take(3).collect::<Vec<_>>(),
                    properties = ?prop_names,
                    "Generic state name - consider adding a pattern"
                );
                format!("State{}Props", prop_names.len())
            }
        } else {
            format!("{}State", to_pascal_case(&blocks[0]))
        };

        // If name clashes with an enum, append "Block" to differentiate
        if generated_states.contains(&state_name) {
            // Replace "State" suffix with "BlockState" to avoid collision
            if state_name.ends_with("State") {
                state_name = format!("{}BlockState", &state_name[..state_name.len() - 5]);
            } else {
                state_name = format!("{}Block", state_name);
            }
            // If still clashes, skip
            if generated_states.contains(&state_name) {
                continue;
            }
        }
        generated_states.insert(state_name.clone());

        // Calculate state count
        let state_count: u32 = shape
            .props
            .iter()
            .map(|p| {
                if p.prop_type == PropType::String {
                    p.string_values.len() as u32
                } else {
                    (p.max - p.min + 1) as u32
                }
            })
            .product();

        writeln!(
            out,
            "/// State shared by: {:?}",
            blocks.iter().take(5).collect::<Vec<_>>()
        )?;
        if blocks.len() > 5 {
            writeln!(out, "/// ... and {} more blocks", blocks.len() - 5)?;
        }
        writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]")?;
        writeln!(out, "pub struct {} {{", state_name)?;

        // Private fields
        for prop in &shape.props {
            let field_name = to_snake_case(&prop.name);
            let field_type = match prop.prop_type {
                PropType::Byte if prop.min == 0 && prop.max == 1 => "bool".to_string(),
                PropType::Byte | PropType::Int => "u8".to_string(),
                PropType::String => to_pascal_case(&to_snake_case(&prop.name)),
            };
            writeln!(out, "    {}: {},", field_name, field_type)?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;

        // impl block with new(), getters
        writeln!(out, "impl {} {{", state_name)?;

        // Generate new() with validation
        let mut params = Vec::new();
        for prop in &shape.props {
            let field_name = to_snake_case(&prop.name);
            let param_type = match prop.prop_type {
                PropType::Byte if prop.min == 0 && prop.max == 1 => "bool".to_string(),
                PropType::Byte | PropType::Int => "u8".to_string(),
                PropType::String => to_pascal_case(&to_snake_case(&prop.name)),
            };
            params.push((field_name, param_type));
        }

        let param_list: String = params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, t))
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(out, "    /// Create a new state with validation.")?;
        writeln!(
            out,
            "    pub fn new({}) -> Result<Self, valentine_bedrock_core::block::StateError> {{",
            param_list
        )?;

        // Validation for non-bool numeric fields
        for prop in &shape.props {
            let field_name = to_snake_case(&prop.name);
            // Only validate non-bool numeric fields
            if prop.prop_type != PropType::String && !(prop.min == 0 && prop.max == 1) {
                let max = (prop.max - prop.min) as u32;
                writeln!(out, "        if {} > {} {{", field_name, max)?;
                writeln!(
                    out,
                    "            return Err(valentine_bedrock_core::block::StateError::OutOfRange {{ field: \"{}\", value: {} as u32, min: 0, max: {} }});",
                    field_name, field_name, max
                )?;
                writeln!(out, "        }}")?;
            }
        }

        let field_names = params
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "        Ok(Self {{ {} }})", field_names)?;
        writeln!(out, "    }}")?;
        writeln!(out)?;

        // Generate getters
        for prop in &shape.props {
            let field_name = to_snake_case(&prop.name);
            let ret_type = match prop.prop_type {
                PropType::Byte if prop.min == 0 && prop.max == 1 => "bool".to_string(),
                PropType::Byte | PropType::Int => "u8".to_string(),
                PropType::String => to_pascal_case(&to_snake_case(&prop.name)),
            };
            writeln!(out, "    /// Get the {} value.", field_name)?;
            writeln!(out, "    #[inline]")?;
            writeln!(
                out,
                "    pub fn {}(&self) -> {} {{ self.{} }}",
                field_name, ret_type, field_name
            )?;
        }

        writeln!(out, "}}")?;
        writeln!(out)?;

        // Default impl using first valid values
        writeln!(out, "impl Default for {} {{", state_name)?;
        writeln!(out, "    fn default() -> Self {{")?;
        write!(out, "        Self {{ ")?;
        for (i, prop) in shape.props.iter().enumerate() {
            let field_name = to_snake_case(&prop.name);
            let default_val = match prop.prop_type {
                PropType::Byte if prop.min == 0 && prop.max == 1 => "false".to_string(),
                PropType::Byte | PropType::Int => "0".to_string(),
                PropType::String => {
                    let enum_name = to_pascal_case(&to_snake_case(&prop.name));
                    format!("{}::default()", enum_name)
                }
            };
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "{}: {}", field_name, default_val)?;
        }
        writeln!(out, " }}")?;
        writeln!(out, "    }}")?;
        writeln!(out, "}}")?;
        writeln!(out)?;

        // Implement BlockState
        writeln!(out, "impl BlockState for {} {{", state_name)?;

        // state_offset
        writeln!(out, "    fn state_offset(&self) -> u32 {{")?;
        writeln!(out, "        let mut offset = 0u32;")?;
        // Only need mut multiplier if there are multiple properties
        if shape.props.len() > 1 {
            writeln!(out, "        let mut multiplier = 1u32;")?;
        } else {
            writeln!(out, "        let multiplier = 1u32;")?;
        }
        for (i, prop) in shape.props.iter().enumerate() {
            let field_name = to_snake_case(&prop.name);
            let range = if prop.prop_type == PropType::String {
                prop.string_values.len() as u32
            } else {
                (prop.max - prop.min + 1) as u32
            };

            writeln!(
                out,
                "        offset += (self.{} as u32) * multiplier;",
                field_name
            )?;
            // Skip multiplier update for last property to avoid unused assignment warning
            if i < shape.props.len() - 1 {
                writeln!(out, "        multiplier *= {};", range)?;
            }
        }
        writeln!(out, "        offset")?;
        writeln!(out, "    }}")?;
        writeln!(out)?;

        // from_offset
        writeln!(out, "    fn from_offset(offset: u32) -> Option<Self> {{")?;
        writeln!(
            out,
            "        if offset >= {} {{ return None; }}",
            state_count
        )?;
        // Only need mut rem if there are multiple properties
        if shape.props.len() > 1 {
            writeln!(out, "        let mut rem = offset;")?;
        } else {
            writeln!(out, "        let rem = offset;")?;
        }
        for (i, prop) in shape.props.iter().enumerate() {
            let field_name = to_snake_case(&prop.name);
            let range = if prop.prop_type == PropType::String {
                prop.string_values.len() as u32
            } else {
                (prop.max - prop.min + 1) as u32
            };

            if prop.prop_type == PropType::Byte && prop.min == 0 && prop.max == 1 {
                writeln!(out, "        let {} = (rem % {}) != 0;", field_name, range)?;
            } else if prop.prop_type == PropType::String {
                let enum_name = to_pascal_case(&to_snake_case(&prop.name));
                writeln!(
                    out,
                    "        let {} = {}::from_raw((rem % {}) as u8)?;",
                    field_name, enum_name, range
                )?;
            } else {
                writeln!(out, "        let {} = (rem % {}) as u8;", field_name, range)?;
            }
            // Skip rem update for last property to avoid unused assignment warning
            if i < shape.props.len() - 1 {
                writeln!(out, "        rem /= {};", range)?;
            }
        }
        writeln!(
            out,
            "        Some(Self {{ {} }})",
            shape
                .props
                .iter()
                .map(|p| to_snake_case(&p.name))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        writeln!(out, "    }}")?;
        writeln!(out)?;

        // state_count
        writeln!(out, "    fn state_count() -> u32 {{ {} }}", state_count)?;
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    Ok(())
}

/// Derive a descriptive state name from block property patterns.
/// Used by both block_states.rs and blocks.rs generators.
/// `prop_names` should be normalized (snake_case, no minecraft: prefix).
/// Returns None if no pattern matches (caller should use suffix detection or fallback).
pub fn derive_state_name(prop_names: &[String]) -> Option<String> {
    // Helper to check if a property name exists
    let has = |name: &str| prop_names.iter().any(|n| n == name);
    let len = prop_names.len();

    // Wall blocks have wall_connection_type_* properties
    if prop_names
        .iter()
        .any(|n| n.starts_with("wall_connection_type"))
    {
        return Some("WallState".to_string());
    }
    // Doors
    if has("door_hinge_bit") && has("open_bit") {
        return Some("DoorState".to_string());
    }
    // Stairs
    if has("weirdo_direction") && has("upside_down_bit") {
        return Some("StairState".to_string());
    }
    // Pillars/logs
    if has("pillar_axis") && len == 1 {
        return Some("PillarState".to_string());
    }
    // Slabs
    if prop_names.iter().any(|n| n.contains("vertical_half")) && len == 1 {
        return Some("SlabState".to_string());
    }
    // Facing (6 directions)
    if has("facing_direction") && len == 1 {
        return Some("FacingState".to_string());
    }
    // Cardinal direction only (4 directions)
    if has("cardinal_direction") && len == 1 {
        return Some("CardinalState".to_string());
    }
    // Trapdoors
    if has("open_bit") && has("direction") && has("upside_down_bit") {
        return Some("TrapdoorState".to_string());
    }
    // Fence gates
    if has("open_bit") && has("direction") && has("in_wall_bit") {
        return Some("FenceGateState".to_string());
    }
    // Buttons
    if has("button_pressed_bit") && has("facing_direction") {
        return Some("ButtonState".to_string());
    }
    // Levers
    if has("open_bit") && has("lever_direction") {
        return Some("LeverState".to_string());
    }
    // Pressure plates (have redstone_signal only)
    if has("redstone_signal") && len == 1 {
        return Some("PressurePlateState".to_string());
    }
    // Torches (torch_facing_direction only)
    if has("torch_facing_direction") && len == 1 {
        return Some("TorchState".to_string());
    }
    // Dispensers/droppers (facing_direction + triggered_bit)
    if has("facing_direction") && has("triggered_bit") && len == 2 {
        return Some("DispenserState".to_string());
    }
    // Leaves (persistent_bit + update_bit)
    if has("persistent_bit") && has("update_bit") && len == 2 {
        return Some("LeavesState".to_string());
    }
    // Standing signs/banners (ground_sign_direction only)
    if has("ground_sign_direction") && len == 1 {
        return Some("StandingSignState".to_string());
    }
    // Petals/flowers (growth + cardinal_direction)
    if has("growth") && has("cardinal_direction") && len == 2 {
        return Some("PetalsState".to_string());
    }
    // Brushable blocks (brushed_progress + hanging)
    if has("brushed_progress") && has("hanging") && len == 2 {
        return Some("BrushableState".to_string());
    }
    // Creaking heart (creaking_heart_state + natural + pillar_axis)
    if has("creaking_heart_state") && has("natural") && has("pillar_axis") {
        return Some("CreakingHeartBlockState".to_string());
    }
    // Candles (candles + lit)
    if has("candles") && has("lit") {
        return Some("CandleState".to_string());
    }
    // Candle cake (lit only - single prop)
    if has("lit") && len == 1 {
        return Some("CandleCakeState".to_string());
    }
    // Lanterns (hanging only - single prop)
    if has("hanging") && len == 1 {
        return Some("LanternState".to_string());
    }
    // Double plants/tall plants (upper_block_bit only)
    if has("upper_block_bit") && len == 1 {
        return Some("DoublePlantState".to_string());
    }
    // Beehives/bee nests (direction + honey_level)
    if has("direction") && has("honey_level") && len == 2 {
        return Some("BeehiveState".to_string());
    }
    // Crops (growth only - wheat, carrots, beetroot, etc.)
    if has("growth") && len == 1 {
        return Some("CropState".to_string());
    }
    // Deprecated pillar blocks (deprecated + pillar_axis - hay_block, bone_block)
    if has("deprecated") && has("pillar_axis") && len == 2 {
        return Some("DeprecatedPillarState".to_string());
    }
    // Simple direction blocks (direction only - loom, element_constructor)
    if has("direction") && len == 1 {
        return Some("DirectionState".to_string());
    }
    // Liquids (liquid_depth only - water, lava)
    if has("liquid_depth") && len == 1 {
        return Some("LiquidState".to_string());
    }
    // Age-based blocks (age only - frosted_ice, nether_wart)
    if has("age") && len == 1 {
        return Some("AgeState".to_string());
    }
    // Block face (block_face only - amethyst buds)
    if has("block_face") && len == 1 {
        return Some("BlockFaceState".to_string());
    }
    // Growing plants (growing_plant_age only - cave vines)
    if has("growing_plant_age") && len == 1 {
        return Some("GrowingPlantState".to_string());
    }
    // Multi-face blocks (multi_face_direction_bits only - glow lichen, sculk vein)
    if has("multi_face_direction_bits") && len == 1 {
        return Some("MultiFaceState".to_string());
    }
    // Mushroom blocks (huge_mushroom_bits only)
    if has("huge_mushroom_bits") && len == 1 {
        return Some("MushroomState".to_string());
    }

    // No pattern matched - caller should use suffix detection or fallback
    None
}

/// Find the common suffix from block names.
/// E.g., ["oak_hanging_sign", "birch_hanging_sign"] → "hanging_sign"
fn find_common_suffix(blocks: &[String]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    // Split each block name into parts
    let parts_list: Vec<Vec<&str>> = blocks
        .iter()
        .map(|b| b.split('_').collect::<Vec<_>>())
        .collect();

    if parts_list.is_empty() {
        return None;
    }

    // Find common suffix by comparing from the end
    let mut common_suffix: Vec<&str> = Vec::new();
    let min_len = parts_list.iter().map(|p| p.len()).min().unwrap_or(0);

    for i in 1..=min_len {
        let suffix_part = parts_list[0][parts_list[0].len() - i];
        let all_match = parts_list
            .iter()
            .all(|p| p.len() >= i && p[p.len() - i] == suffix_part);

        if all_match {
            common_suffix.insert(0, suffix_part);
        } else {
            break;
        }
    }

    // Need at least 1 meaningful part
    if common_suffix.is_empty() {
        return None;
    }

    // Filter out generic words that don't make good state names
    let generic = ["block", "blocks"];
    let result: Vec<_> = common_suffix
        .into_iter()
        .filter(|s| !generic.contains(s))
        .collect();

    if result.is_empty() {
        return None;
    }

    Some(result.join("_"))
}
