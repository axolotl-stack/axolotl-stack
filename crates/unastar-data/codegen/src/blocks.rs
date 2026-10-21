//! Block registry code generation from normalized KDL artifacts.

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
    })
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

    #[test]
    fn parses_block_node_with_default_state_id() {
        let doc: KdlDocument = r#"block "minecraft:stone" id=1 name="Stone" hardness=1.5 resistance=6.0 is_transparent=#false emit_light=0 filter_light=15 min_state_id=0 max_state_id=1 default_state_id=0 state_id_count=2"#
            .parse()
            .expect("parse block");

        let block = parse_block_node(&doc.nodes()[0]).expect("parse block node");

        assert_eq!(block.identifier, "minecraft:stone");
        assert_eq!(block.id, 1);
        assert_eq!(block.default_state_id, 0);
        assert_eq!(block.state_id_count, 2);
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
        }
    }
}
