//! Block registry code generation from normalized KDL artifacts.

use base64::Engine;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, PartialEq)]
struct ParsedBlock {
    identifier: String,
    id: u32,
    name: String,
    hardness: f32,
    resistance: f32,
    is_transparent: bool,
    emit_light: u8,
    filter_light: u8,
    min_state_id: u32,
    max_state_id: u32,
    default_state_id: u32,
    state_id_count: u32,
    tags: Vec<String>,
    physics: NativePhysics,
    states: StateTable,
    /// `StartGame` network NBT definition of a vanilla data-driven block.
    server_definition: Option<Vec<u8>>,
}

/// Block-level physical facts projected from the native properties table
/// (`native_properties_json` on each KDL block node).
#[derive(Debug, Clone, PartialEq)]
struct NativePhysics {
    friction: f32,
    material: u8,
    blocks_motion: bool,
    liquid: bool,
    solid: bool,
    falling: bool,
    requires_correct_tool_for_drops: bool,
    visual_shape: [f32; 6],
}

/// Typed native block-state value (`{"byte":..}`, `{"int":..}`, `{"string":..}`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum StateValue {
    Byte(u8),
    Int(i32),
    Str(String),
}

/// Per-block state property table: property names (sorted), the distinct
/// values of each property in first-appearance canonical order, and for every
/// canonical state (in runtime-ID order) the index of its value per property.
#[derive(Debug, Clone, Default, PartialEq)]
struct StateTable {
    properties: Vec<(String, Vec<StateValue>)>,
    /// Row-major: `state_offset * properties.len() + property_index`.
    value_indices: Vec<u8>,
}

pub fn generate_blocks(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let path = input_dir.join("blocks.kdl");
    if !path.exists() {
        info!("No blocks.kdl found at {}, skipping", path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("blocks.kdl parse error: {}", e))?;

    let mut blocks = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == "block" {
            blocks.push(parse_block_node(node)?);
        }
    }

    blocks.sort_by(|a, b| {
        a.min_state_id
            .cmp(&b.min_state_id)
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
    validate_blocks(&blocks)?;

    info!("Parsed {} block registry rows", blocks.len());

    generate_blocks_module(&blocks, output_dir)
}

fn parse_block_node(node: &KdlNode) -> miette::Result<ParsedBlock> {
    let identifier = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("block node missing identifier"))?
        .to_string();

    let id = property_u32(node, "id", &identifier)?;
    let name = node
        .get("name")
        .and_then(|entry| entry.as_string())
        .ok_or_else(|| miette::miette!("block {} missing name", identifier))?
        .to_string();
    let hardness = property_f32(node, "hardness", &identifier)?;
    let resistance = property_f32(node, "resistance", &identifier)?;
    let is_transparent = node
        .get("is_transparent")
        .and_then(|entry| entry.as_bool())
        .ok_or_else(|| miette::miette!("block {} missing is_transparent", identifier))?;
    let emit_light = property_u8(node, "emit_light", &identifier)?;
    let filter_light = property_u8(node, "filter_light", &identifier)?;
    let min_state_id = property_u32(node, "min_state_id", &identifier)?;
    let max_state_id = property_u32(node, "max_state_id", &identifier)?;
    let default_state_id = property_u32(node, "default_state_id", &identifier)?;
    let state_id_count = property_u32(node, "state_id_count", &identifier)?;
    let tags = node
        .children()
        .into_iter()
        .flat_map(|doc| doc.nodes())
        .filter(|n| n.name().value() == "tag")
        .map(|n| {
            n.get(0)
                .and_then(|v| v.as_string())
                .map(str::to_owned)
                .ok_or_else(|| miette::miette!("block {identifier} has invalid tag"))
        })
        .collect::<miette::Result<Vec<_>>>()?;
    let physics_json = node
        .get("native_properties_json")
        .and_then(|entry| entry.as_string())
        .ok_or_else(|| miette::miette!("block {identifier} missing native_properties_json"))?;
    let physics = parse_native_physics(physics_json, &identifier)?;
    let states = parse_state_table(node, &identifier, min_state_id, state_id_count)?;
    let server_definition = node
        .get("server_definition_nbt")
        .map(|value| {
            let encoded = value.as_string().ok_or_else(|| {
                miette::miette!("block {identifier} server_definition_nbt must be a string")
            })?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| miette::miette!("block {identifier} server_definition_nbt: {e}"))?;
            if !bytes.starts_with(&[0x0a, 0x00]) || bytes.last() != Some(&0) {
                return Err(miette::miette!(
                    "block {identifier} server_definition_nbt is not an unnamed network NBT compound"
                ));
            }
            Ok(bytes)
        })
        .transpose()?;

    Ok(ParsedBlock {
        identifier,
        id,
        name,
        hardness,
        resistance,
        is_transparent,
        emit_light,
        filter_light,
        min_state_id,
        max_state_id,
        default_state_id,
        state_id_count,
        tags,
        physics,
        states,
        server_definition,
    })
}

fn parse_native_physics(json: &str, identifier: &str) -> miette::Result<NativePhysics> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| miette::miette!("block {identifier} native_properties_json: {e}"))?;
    let field = |name: &str| {
        value
            .get(name)
            .ok_or_else(|| miette::miette!("block {identifier} native properties lack {name}"))
    };
    let flag = |v: &serde_json::Value, name: &str| {
        v.get(name)
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| miette::miette!("block {identifier} native {name} is not a bool"))
    };
    let material = field("material")?;
    let shape = field("visualShape")?
        .as_array()
        .filter(|values| values.len() == 6)
        .ok_or_else(|| miette::miette!("block {identifier} visualShape is not six numbers"))?;
    let mut visual_shape = [0.0f32; 6];
    for (slot, value) in visual_shape.iter_mut().zip(shape) {
        *slot = value
            .as_f64()
            .ok_or_else(|| miette::miette!("block {identifier} visualShape is not numeric"))?
            as f32;
    }
    Ok(NativePhysics {
        friction: field("friction")?
            .as_f64()
            .ok_or_else(|| miette::miette!("block {identifier} friction is not numeric"))?
            as f32,
        material: material
            .get("type")
            .and_then(serde_json::Value::as_u64)
            .and_then(|ty| u8::try_from(ty).ok())
            .ok_or_else(|| miette::miette!("block {identifier} material.type is not a u8"))?,
        blocks_motion: flag(material, "blocksMotion")?,
        liquid: flag(material, "liquid")?,
        solid: flag(&value, "solid")?,
        falling: flag(&value, "falling")?,
        requires_correct_tool_for_drops: flag(&value, "requiresCorrectToolForDrops")?,
        visual_shape,
    })
}

fn parse_state_value(value: &serde_json::Value, identifier: &str) -> miette::Result<StateValue> {
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(|| miette::miette!("block {identifier} has a malformed state value"))?;
    let (kind, raw) = object.iter().next().expect("one entry");
    match kind.as_str() {
        "byte" => raw
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .map(StateValue::Byte),
        "int" => raw
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(StateValue::Int),
        "string" => raw.as_str().map(|v| StateValue::Str(v.to_owned())),
        _ => None,
    }
    .ok_or_else(|| miette::miette!("block {identifier} has unsupported state value {value}"))
}

/// Build the per-block state table from the `state` children, which are in
/// canonical runtime-ID order. Fails if a state ID is out of order or a
/// state's property set differs from the block's first state.
fn parse_state_table(
    node: &KdlNode,
    identifier: &str,
    min_state_id: u32,
    state_id_count: u32,
) -> miette::Result<StateTable> {
    let mut table = StateTable::default();
    let state_nodes: Vec<&KdlNode> = node
        .children()
        .into_iter()
        .flat_map(|doc| doc.nodes())
        .filter(|n| n.name().value() == "state")
        .collect();
    if state_nodes.is_empty() {
        return Ok(table);
    }
    if state_nodes.len() != state_id_count as usize {
        return Err(miette::miette!(
            "block {identifier} lists {} states, expected {state_id_count}",
            state_nodes.len()
        ));
    }
    for (offset, state) in state_nodes.into_iter().enumerate() {
        let state_id = state
            .get(0)
            .and_then(|v| v.as_integer())
            .ok_or_else(|| miette::miette!("block {identifier} state without ID"))?;
        if state_id != i128::from(min_state_id) + offset as i128 {
            return Err(miette::miette!(
                "block {identifier} state {state_id} is out of canonical order"
            ));
        }
        let json = state
            .get("properties_json")
            .and_then(|v| v.as_string())
            .ok_or_else(|| miette::miette!("block {identifier} state {state_id} properties"))?;
        let properties: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json)
            .map_err(|e| miette::miette!("block {identifier} state {state_id}: {e}"))?;
        if offset == 0 {
            let mut names: Vec<&String> = properties.keys().collect();
            names.sort();
            table.properties = names
                .into_iter()
                .map(|name| (name.clone(), Vec::new()))
                .collect();
        }
        if properties.len() != table.properties.len() {
            return Err(miette::miette!(
                "block {identifier} state {state_id} has a different property set"
            ));
        }
        for (name, values) in &mut table.properties {
            let raw = properties.get(name).ok_or_else(|| {
                miette::miette!("block {identifier} state {state_id} lacks property {name}")
            })?;
            let value = parse_state_value(raw, identifier)?;
            let index = match values.iter().position(|known| *known == value) {
                Some(index) => index,
                None => {
                    values.push(value);
                    values.len() - 1
                }
            };
            table.value_indices.push(u8::try_from(index).map_err(|_| {
                miette::miette!("block {identifier} property {name} has more than 256 values")
            })?);
        }
    }
    Ok(table)
}

fn property_u32(node: &KdlNode, name: &str, identifier: &str) -> miette::Result<u32> {
    let value = node
        .get(name)
        .and_then(|entry| entry.as_integer())
        .ok_or_else(|| miette::miette!("block {} missing {}", identifier, name))?;
    u32::try_from(value).map_err(|_| {
        miette::miette!(
            "block {} property {} value {} is out of u32 range",
            identifier,
            name,
            value
        )
    })
}

fn property_u8(node: &KdlNode, name: &str, identifier: &str) -> miette::Result<u8> {
    let value = node
        .get(name)
        .and_then(|entry| entry.as_integer())
        .ok_or_else(|| miette::miette!("block {} missing {}", identifier, name))?;
    u8::try_from(value).map_err(|_| {
        miette::miette!(
            "block {} property {} value {} is out of u8 range",
            identifier,
            name,
            value
        )
    })
}

fn property_f32(node: &KdlNode, name: &str, identifier: &str) -> miette::Result<f32> {
    let value = node
        .get(name)
        .and_then(|entry| {
            entry
                .as_float()
                .or_else(|| entry.as_integer().map(|v| v as f64))
        })
        .ok_or_else(|| miette::miette!("block {} missing {}", identifier, name))?;
    if !value.is_finite() || value < f32::MIN as f64 || value > f32::MAX as f64 {
        return Err(miette::miette!(
            "block {} property {} value {} is out of f32 range",
            identifier,
            name,
            value
        ));
    }
    Ok(value as f32)
}

fn validate_blocks(blocks: &[ParsedBlock]) -> miette::Result<()> {
    if blocks.is_empty() {
        return Err(miette::miette!(
            "blocks.kdl did not contain any block nodes"
        ));
    }

    let mut seen_identifiers = HashSet::new();
    let mut next_expected_min = None;

    for block in blocks {
        if block.identifier.trim().is_empty() {
            return Err(miette::miette!("block identifier must be non-empty"));
        }
        if !seen_identifiers.insert(block.identifier.as_str()) {
            return Err(miette::miette!(
                "duplicate block identifier {}",
                block.identifier
            ));
        }
        if block.max_state_id < block.min_state_id {
            return Err(miette::miette!(
                "block {} has invalid state range {}..={}",
                block.identifier,
                block.min_state_id,
                block.max_state_id
            ));
        }
        if next_expected_min.is_none() && block.min_state_id != 0 {
            return Err(miette::miette!(
                "first block {} starts at state ID {}, expected canonical state ID 0",
                block.identifier,
                block.min_state_id
            ));
        }
        if !(block.min_state_id..=block.max_state_id).contains(&block.default_state_id) {
            return Err(miette::miette!(
                "block {} default_state_id {} is outside range {}..={}",
                block.identifier,
                block.default_state_id,
                block.min_state_id,
                block.max_state_id
            ));
        }
        let expected_count = block.max_state_id - block.min_state_id + 1;
        if block.state_id_count != expected_count {
            return Err(miette::miette!(
                "block {} state_id_count {} does not match canonical range count {}",
                block.identifier,
                block.state_id_count,
                expected_count
            ));
        }
        if block.emit_light > 15 || block.filter_light > 15 {
            return Err(miette::miette!(
                "block {} has light values outside 0..=15: emit_light={} filter_light={}",
                block.identifier,
                block.emit_light,
                block.filter_light
            ));
        }
        if let Some(next_min) = next_expected_min
            && block.min_state_id != next_min
        {
            return Err(miette::miette!(
                "block {} starts at state ID {}, expected contiguous state ID {}",
                block.identifier,
                block.min_state_id,
                next_min
            ));
        }
        next_expected_min = block.max_state_id.checked_add(1);
    }

    Ok(())
}

fn generate_blocks_module(blocks: &[ParsedBlock], output_dir: &Path) -> miette::Result<()> {
    let entries: Vec<TokenStream> = blocks
        .iter()
        .map(|block| {
            let id = block.id;
            let identifier = &block.identifier;
            let name = &block.name;
            let hardness = block.hardness;
            let resistance = block.resistance;
            let is_transparent = block.is_transparent;
            let emit_light = block.emit_light;
            let filter_light = block.filter_light;
            let tags = &block.tags;
            let min_state_id = block.min_state_id;
            let max_state_id = block.max_state_id;
            let default_state_id = block.default_state_id;
            let state_id_count = block.state_id_count;
            let NativePhysics {
                friction,
                material,
                blocks_motion,
                liquid,
                solid,
                falling,
                requires_correct_tool_for_drops,
                visual_shape,
            } = &block.physics;
            let properties = block.states.properties.iter().map(|(name, values)| {
                let values = values.iter().map(|value| match value {
                    StateValue::Byte(v) => quote! { StateValue::Byte(#v) },
                    StateValue::Int(v) => quote! { StateValue::Int(#v) },
                    StateValue::Str(v) => quote! { StateValue::Str(#v) },
                });
                quote! { BlockProperty { name: #name, values: &[#(#values),*] } }
            });
            let state_values = proc_macro2::Literal::byte_string(&block.states.value_indices);
            let server_definition = match &block.server_definition {
                Some(bytes) => {
                    let bytes = proc_macro2::Literal::byte_string(bytes);
                    quote! { Some(#bytes) }
                }
                None => quote! { None },
            };
            quote! {
                BlockData {
                    id: #id,
                    identifier: #identifier,
                    name: #name,
                    source: SOURCE,
                    state_id_count: #state_id_count,
                    min_state_id: #min_state_id,
                    max_state_id: #max_state_id,
                    default_state_id: #default_state_id,
                    hardness: #hardness,
                    tags: &[#(#tags),*],
                    resistance: #resistance,
                    is_transparent: #is_transparent,
                    emit_light: #emit_light,
                    filter_light: #filter_light,
                    friction: #friction,
                    material: #material,
                    blocks_motion: #blocks_motion,
                    liquid: #liquid,
                    solid: #solid,
                    falling: #falling,
                    requires_correct_tool_for_drops: #requires_correct_tool_for_drops,
                    visual_shape: [#(#visual_shape),*],
                    properties: &[#(#properties),*],
                    state_values: #state_values,
                    server_definition: #server_definition,
                }
            }
        })
        .collect();
    let identifier_match_arms: Vec<TokenStream> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let identifier = &block.identifier;
            quote! { #identifier => Some(&ALL_BLOCKS[#index]) }
        })
        .collect();
    let runtime_id_match_arms: Vec<TokenStream> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let min_state_id = block.min_state_id;
            let max_state_id = block.max_state_id;
            quote! { #min_state_id..=#max_state_id => Some(&ALL_BLOCKS[#index]) }
        })
        .collect();
    let count = entries.len();

    let code = quote! {
        //! Generated block registry data from normalized block artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! Native BDS 1.26.51 palette and physical measurements.
        //! Default state selection remains the explicitly weak first-palette-state policy.

        /// Source family for all rows in this generated module.
        pub const SOURCE: &str = "protocolgen_native_1_26_51";

        /// Source-attributed block registry row.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct BlockData {
            /// Registry identity: first canonical runtime state ID, not a legacy block ID.
            pub id: u32,
            /// Namespaced block identifier.
            pub identifier: &'static str,
            /// Native identifier (no translated display-name source is available).
            pub name: &'static str,
            /// Source family for this definition.
            pub source: &'static str,
            /// Native BDS block tags.
            pub tags: &'static [&'static str],
            /// Number of canonical runtime state IDs in this block's range.
            pub state_id_count: u32,
            /// Minimum canonical runtime state ID.
            pub min_state_id: u32,
            /// Maximum canonical runtime state ID.
            pub max_state_id: u32,
            /// First canonical state; weak default-selection policy, not a measured placement default.
            pub default_state_id: u32,
            /// Native destroy speed of the first canonical state.
            pub hardness: f32,
            /// Native explosion resistance of the first canonical state.
            pub resistance: f32,
            /// Inverse native opaque-full-block flag of the first canonical state.
            pub is_transparent: bool,
            /// Native emitted light level of the first canonical state.
            pub emit_light: u8,
            /// Native light damping of the first canonical state.
            pub filter_light: u8,
            /// Native block friction (`friction`; 0.6 for ordinary blocks, 0.98 ice).
            pub friction: f32,
            /// Native material type ordinal (`material.type`).
            pub material: u8,
            /// Native `material.blocksMotion`: entities collide with this block.
            pub blocks_motion: bool,
            /// Native `material.liquid`.
            pub liquid: bool,
            /// Native block-level `solid` flag.
            pub solid: bool,
            /// Native `falling` flag (sand, gravel, concrete powder, ...).
            pub falling: bool,
            /// Native `requiresCorrectToolForDrops`.
            pub requires_correct_tool_for_drops: bool,
            /// Native `visualShape` of the first canonical state as
            /// `[min_x, min_y, min_z, max_x, max_y, max_z]` in block units.
            pub visual_shape: [f32; 6],
            /// State properties sorted by name, each with its values in
            /// first-appearance canonical order.
            pub properties: &'static [BlockProperty],
            /// Row-major value indices: for canonical state offset `s` and
            /// property index `p`, `state_values[s * properties.len() + p]`
            /// indexes `properties[p].values`.
            pub state_values: &'static [u8],
            /// Network NBT block definition `StartGame` sends for a vanilla
            /// data-driven block (behavior pack `blocks/`); `None` for built-in
            /// blocks. The client's sequential block ids cover built-in blocks
            /// plus defined blocks only.
            pub server_definition: Option<&'static [u8]>,
        }

        /// One native block-state property.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct BlockProperty {
            /// Native property name, e.g. `minecraft:cardinal_direction`.
            pub name: &'static str,
            /// Distinct values in first-appearance canonical order.
            pub values: &'static [StateValue],
        }

        /// Typed native block-state value.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum StateValue {
            /// NBT byte property (booleans such as `open_bit`).
            Byte(u8),
            /// NBT int property.
            Int(i32),
            /// NBT string property.
            Str(&'static str),
        }

        impl BlockData {
            fn state_row(&self, runtime_id: u32) -> Option<&'static [u8]> {
                if !(self.min_state_id..=self.max_state_id).contains(&runtime_id) {
                    return None;
                }
                let width = self.properties.len();
                let start = (runtime_id - self.min_state_id) as usize * width;
                self.state_values.get(start..start + width)
            }

            /// Value of `property` in canonical state `runtime_id` of this block.
            pub fn state_value(&self, runtime_id: u32, property: &str) -> Option<StateValue> {
                let row = self.state_row(runtime_id)?;
                let index = self.properties.iter().position(|p| p.name == property)?;
                self.properties[index].values.get(usize::from(row[index])).copied()
            }

            /// The state equal to `runtime_id` except that `property` is `value`.
            /// `None` if the block lacks the property or no canonical state matches.
            pub fn with_state_value(
                &self,
                runtime_id: u32,
                property: &str,
                value: StateValue,
            ) -> Option<u32> {
                let row = self.state_row(runtime_id)?;
                let index = self.properties.iter().position(|p| p.name == property)?;
                let wanted = u8::try_from(
                    self.properties[index].values.iter().position(|v| *v == value)?,
                )
                .ok()?;
                (self.min_state_id..=self.max_state_id).find(|&candidate| {
                    self.state_row(candidate).is_some_and(|other| {
                        other.iter().enumerate().all(|(i, v)| {
                            if i == index { *v == wanted } else { *v == row[i] }
                        })
                    })
                })
            }
        }

        /// All generated block registry rows sorted by canonical runtime state range.
        pub static ALL_BLOCKS: [BlockData; #count] = [
            #(#entries),*
        ];

        /// Look up a block by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static BlockData> {
            match identifier {
                #(#identifier_match_arms,)*
                _ => None,
            }
        }

        /// Look up a block by canonical runtime state ID.
        pub fn by_runtime_id(runtime_id: u32) -> Option<&'static BlockData> {
            match runtime_id {
                #(#runtime_id_match_arms,)*
                _ => None,
            }
        }
    };

    std::fs::write(output_dir.join("blocks.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write blocks.rs: {}", e))?;

    Ok(())
}

fn format_code(code: TokenStream) -> miette::Result<String> {
    let file =
        syn::parse2(code).map_err(|e| miette::miette!("Failed to parse generated code: {}", e))?;
    Ok(prettyplease::unparse(&file))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE: &str = r#"native_properties_json="{\"friction\":0.98,\"material\":{\"type\":13,\"blocksMotion\":true,\"liquid\":false},\"solid\":true,\"falling\":false,\"requiresCorrectToolForDrops\":true,\"visualShape\":[0,0,0,1,0.5,1]}""#;

    #[test]
    fn parses_block_node_with_default_state_id() {
        let doc: KdlDocument = format!(r#"block "minecraft:stone" id=1 name="Stone" hardness=1.5 resistance=6.0 is_transparent=#false emit_light=0 filter_light=15 min_state_id=0 max_state_id=1 default_state_id=0 state_id_count=2 {NATIVE}"#)
            .parse()
            .expect("parse block");

        let block = parse_block_node(&doc.nodes()[0]).expect("parse block node");

        assert_eq!(block.identifier, "minecraft:stone");
        assert_eq!(block.id, 1);
        assert_eq!(block.default_state_id, 0);
        assert_eq!(block.state_id_count, 2);
        assert_eq!(block.physics.friction, 0.98);
        assert!(block.physics.blocks_motion && block.physics.requires_correct_tool_for_drops);
        assert_eq!(block.physics.visual_shape[4], 0.5);
    }

    #[test]
    fn block_without_native_properties_is_rejected() {
        let doc: KdlDocument = r#"block "minecraft:stone" id=1 name="Stone" hardness=1.5 resistance=6.0 is_transparent=#false emit_light=0 filter_light=15 min_state_id=0 max_state_id=1 default_state_id=0 state_id_count=2"#
            .parse()
            .expect("parse block");
        let error = parse_block_node(&doc.nodes()[0]).expect_err("native properties required");
        assert!(error.to_string().contains("native_properties_json"));
    }

    #[test]
    fn state_table_indexes_values_in_canonical_order() {
        let doc: KdlDocument = format!(
            r#"block "minecraft:candle" id=2 name="c" hardness=0.1 resistance=0.1 is_transparent=#true emit_light=0 filter_light=0 min_state_id=2 max_state_id=5 default_state_id=2 state_id_count=4 {NATIVE} {{
    state 2 properties_json="{{\"lit\":{{\"byte\":0}},\"candles\":{{\"int\":0}}}}"
    state 3 properties_json="{{\"lit\":{{\"byte\":0}},\"candles\":{{\"int\":1}}}}"
    state 4 properties_json="{{\"lit\":{{\"byte\":1}},\"candles\":{{\"int\":0}}}}"
    state 5 properties_json="{{\"lit\":{{\"byte\":1}},\"candles\":{{\"int\":1}}}}"
}}"#
        )
        .parse()
        .expect("parse block");
        let block = parse_block_node(&doc.nodes()[0]).expect("parse block node");
        assert_eq!(
            block.states.properties,
            vec![
                (
                    "candles".to_string(),
                    vec![StateValue::Int(0), StateValue::Int(1)]
                ),
                (
                    "lit".to_string(),
                    vec![StateValue::Byte(0), StateValue::Byte(1)]
                ),
            ]
        );
        assert_eq!(block.states.value_indices, vec![0, 0, 1, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn out_of_order_states_are_rejected() {
        let doc: KdlDocument = format!(
            r#"block "minecraft:candle" id=0 name="c" hardness=0.1 resistance=0.1 is_transparent=#true emit_light=0 filter_light=0 min_state_id=0 max_state_id=1 default_state_id=0 state_id_count=2 {NATIVE} {{
    state 1 properties_json="{{}}"
    state 0 properties_json="{{}}"
}}"#
        )
        .parse()
        .expect("parse block");
        let error = parse_block_node(&doc.nodes()[0]).expect_err("order is enforced");
        assert!(error.to_string().contains("out of canonical order"));
    }

    #[test]
    fn rejects_duplicate_identifiers() {
        let blocks = vec![block("minecraft:a", 0, 0), block("minecraft:a", 1, 1)];

        let error = validate_blocks(&blocks).expect_err("duplicate identifier is invalid");

        assert!(error.to_string().contains("duplicate block identifier"));
    }

    #[test]
    fn rejects_non_contiguous_runtime_ranges() {
        let blocks = vec![block("minecraft:a", 0, 0), block("minecraft:b", 2, 2)];

        let error = validate_blocks(&blocks).expect_err("runtime gap is invalid");

        assert!(error.to_string().contains("expected contiguous state ID"));
    }

    #[test]
    fn rejects_default_state_id_outside_range() {
        let mut blocks = vec![block("minecraft:a", 0, 4)];
        blocks[0].default_state_id = 5;

        let error = validate_blocks(&blocks).expect_err("default state outside range is invalid");

        assert!(error.to_string().contains("outside range"));
    }

    #[test]
    fn rejects_non_zero_first_runtime_id() {
        let blocks = vec![block("minecraft:a", 1, 1)];

        let error = validate_blocks(&blocks).expect_err("first runtime ID should start at zero");

        assert!(error.to_string().contains("expected canonical state ID 0"));
    }

    #[test]
    fn rejects_light_values_outside_bedrock_range() {
        let mut blocks = vec![block("minecraft:a", 0, 0)];
        blocks[0].emit_light = 16;

        let error = validate_blocks(&blocks).expect_err("invalid light value is rejected");

        assert!(error.to_string().contains("outside 0..=15"));
    }

    fn block(identifier: &str, min_state_id: u32, max_state_id: u32) -> ParsedBlock {
        ParsedBlock {
            identifier: identifier.to_string(),
            id: min_state_id,
            name: identifier.to_string(),
            hardness: 0.0,
            resistance: 0.0,
            is_transparent: false,
            emit_light: 0,
            filter_light: 0,
            min_state_id,
            max_state_id,
            default_state_id: min_state_id,
            tags: Vec::new(),
            state_id_count: max_state_id - min_state_id + 1,
            physics: NativePhysics {
                friction: 0.6,
                material: 0,
                blocks_motion: false,
                liquid: false,
                solid: false,
                falling: false,
                requires_correct_tool_for_drops: false,
                visual_shape: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            },
            states: StateTable::default(),
            server_definition: None,
        }
    }
}
