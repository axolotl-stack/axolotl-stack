//! Item registry code generation from normalized KDL artifacts.

use heck::ToShoutySnakeCase;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedItem {
    identifier: String,
    network_id: i32,
    component_based: bool,
    version: i32,
    max_stack_size: u8,
    max_stack_size_source: String,
}

pub fn generate_items(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let path = input_dir.join("items.kdl");
    if !path.exists() {
        info!("No items.kdl found at {}, skipping", path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("items.kdl parse error: {}", e))?;

    let mut items = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == "item" {
            items.push(parse_item_node(node)?);
        }
    }

    items.sort_by(|a, b| {
        a.identifier
            .cmp(&b.identifier)
            .then_with(|| a.network_id.cmp(&b.network_id))
    });
    validate_items(&items)?;

    info!("Parsed {} item registry rows", items.len());

    generate_items_module(&items, output_dir)
}

fn parse_item_node(node: &KdlNode) -> miette::Result<ParsedItem> {
    let identifier = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("item node missing identifier"))?
        .to_string();

    let network_id = node
        .get("runtime_id")
        .and_then(|entry| entry.as_integer())
        .ok_or_else(|| miette::miette!("item {} missing runtime_id", identifier))
        .and_then(|value| {
            i32::try_from(value).map_err(|_| {
                miette::miette!(
                    "item {} runtime_id {} is out of i32 range",
                    identifier,
                    value
                )
            })
        })?;

    let component_based = node
        .get("component_based")
        .and_then(|entry| entry.as_bool())
        .ok_or_else(|| miette::miette!("item {} missing component_based", identifier))?;

    let version = node
        .get("version")
        .and_then(|entry| entry.as_integer())
        .ok_or_else(|| miette::miette!("item {} missing version", identifier))
        .and_then(|value| {
            i32::try_from(value).map_err(|_| {
                miette::miette!("item {} version {} is out of i32 range", identifier, value)
            })
        })?;

    let max_stack_size = node
        .get("max_stack_size")
        .and_then(|entry| entry.as_integer())
        .map(u8::try_from)
        .transpose()
        .map_err(|_| miette::miette!("item {} max_stack_size is out of u8 range", identifier))?
        .unwrap_or(64);
    if max_stack_size == 0 {
        return Err(miette::miette!(
            "item {} max_stack_size must be greater than zero",
            identifier
        ));
    }

    let max_stack_size_source = node
        .get("max_stack_size_source")
        .and_then(|entry| entry.as_string())
        .unwrap_or("unsourced_default")
        .to_string();

    Ok(ParsedItem {
        identifier,
        network_id,
        component_based,
        version,
        max_stack_size,
        max_stack_size_source,
    })
}

fn validate_items(items: &[ParsedItem]) -> miette::Result<()> {
    if items.is_empty() {
        return Err(miette::miette!("items.kdl did not contain any item nodes"));
    }

    let mut seen_identifiers = HashSet::new();
    let mut seen_network_ids = HashSet::new();
    for item in items {
        if item.identifier.trim().is_empty() {
            return Err(miette::miette!("item identifier must be non-empty"));
        }
        if item.max_stack_size_source.trim().is_empty() {
            return Err(miette::miette!(
                "item {} max_stack_size_source must be non-empty",
                item.identifier
            ));
        }
        if !seen_identifiers.insert(item.identifier.as_str()) {
            return Err(miette::miette!(
                "duplicate item identifier {}",
                item.identifier
            ));
        }
        if !seen_network_ids.insert(item.network_id) {
            return Err(miette::miette!(
                "duplicate item network_id {} in items.kdl",
                item.network_id
            ));
        }
        if i16::try_from(item.network_id).is_err() {
            return Err(miette::miette!(
                "item {} network_id {} is out of i16 packet range",
                item.identifier,
                item.network_id
            ));
        }
    }

    Ok(())
}

fn generate_items_module(items: &[ParsedItem], output_dir: &Path) -> miette::Result<()> {
    let entries: Vec<TokenStream> = items
        .iter()
        .enumerate()
        .map(|(id, item)| {
            let const_name = item_const_ident(&item.identifier);
            let id = id as u32;
            let identifier = &item.identifier;
            let network_id = item.network_id;
            let component_based = item.component_based;
            let version = item.version;
            let max_stack_size = item.max_stack_size;
            let max_stack_size_source = &item.max_stack_size_source;
            quote! {
                pub const #const_name: ItemData = ItemData {
                    id: #id,
                    identifier: #identifier,
                    network_id: #network_id,
                    component_based: #component_based,
                    version: #version,
                    max_stack_size: #max_stack_size,
                    max_stack_size_source: #max_stack_size_source,
                };
            }
        })
        .collect();

    let all_refs: Vec<TokenStream> = items
        .iter()
        .map(|item| {
            let const_name = item_const_ident(&item.identifier);
            quote! { #const_name }
        })
        .collect();
    let identifier_match_arms: Vec<TokenStream> = items
        .iter()
        .map(|item| {
            let identifier = &item.identifier;
            let const_name = item_const_ident(identifier);
            quote! { #identifier => Some(&#const_name) }
        })
        .collect();
    let network_id_match_arms: Vec<TokenStream> = items
        .iter()
        .map(|item| {
            let network_id = item.network_id;
            let const_name = item_const_ident(&item.identifier);
            quote! { #network_id => Some(&#const_name) }
        })
        .collect();
    let count = all_refs.len();

    let code = quote! {
        //! Generated item registry data from normalized PMMP/BedrockData artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! Stack sizes come from behavior-pack item components where present,
        //! otherwise they are explicit unsourced defaults for follow-up source
        //! replacement.

        /// Source-attributed item registry row.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ItemData {
            /// Internal registry ID assigned by deterministic codegen order.
            ///
            /// This is not a Bedrock packet/network item ID.
            pub id: u32,
            /// Namespaced item identifier.
            pub identifier: &'static str,
            /// Signed protocol network ID from required item registry data.
            pub network_id: i32,
            /// Whether the item is component-based in Bedrock's item registry.
            pub component_based: bool,
            /// Bedrock item registry version discriminator.
            pub version: i32,
            /// Maximum stack size from sourced behavior-pack item components,
            /// or an explicit unsourced default when the current sources lack
            /// a stronger value.
            pub max_stack_size: u8,
            /// Field-level source for `max_stack_size`.
            pub max_stack_size_source: &'static str,
        }

        #(#entries)*

        /// All generated item registry rows sorted by identifier.
        pub static ALL_ITEMS: [ItemData; #count] = [
            #(#all_refs),*
        ];

        /// Look up an item by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static ItemData> {
            match identifier {
                #(#identifier_match_arms,)*
                _ => None,
            }
        }

        /// Look up an item by signed protocol network ID.
        pub fn by_network_id(network_id: i32) -> Option<&'static ItemData> {
            match network_id {
                #(#network_id_match_arms,)*
                _ => None,
            }
        }
    };

    std::fs::write(output_dir.join("items.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write items.rs: {}", e))?;

    Ok(())
}

fn item_const_ident(identifier: &str) -> proc_macro2::Ident {
    let name = identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .to_shouty_snake_case();
    format_ident!("{}", name)
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
    fn parses_item_node() {
        let doc: KdlDocument =
            r#"item minecraft:apple runtime_id=285 component_based=#true version=1"#
                .parse()
                .expect("parse item");

        let item = parse_item_node(&doc.nodes()[0]).expect("parse item node");

        assert_eq!(item.identifier, "minecraft:apple");
        assert_eq!(item.network_id, 285);
        assert!(item.component_based);
        assert_eq!(item.version, 1);
        assert_eq!(item.max_stack_size, 64);
        assert_eq!(item.max_stack_size_source, "unsourced_default");
    }

    #[test]
    fn parses_sourced_item_stack_size() {
        let doc: KdlDocument = r#"item minecraft:honey_bottle runtime_id=737 component_based=#false version=2 max_stack_size=16 max_stack_size_source="vanilla_behavior_pack""#
            .parse()
            .expect("parse item");

        let item = parse_item_node(&doc.nodes()[0]).expect("parse item node");

        assert_eq!(item.identifier, "minecraft:honey_bottle");
        assert_eq!(item.max_stack_size, 16);
        assert_eq!(item.max_stack_size_source, "vanilla_behavior_pack");
    }

    #[test]
    fn generated_lookups_use_direct_matches() {
        let items = vec![ParsedItem {
            identifier: "minecraft:honey_bottle".to_string(),
            network_id: 737,
            component_based: false,
            version: 2,
            max_stack_size: 16,
            max_stack_size_source: "vanilla_behavior_pack".to_string(),
        }];
        let temp_dir = std::env::temp_dir().join(format!(
            "unastar-items-codegen-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");

        generate_items_module(&items, &temp_dir).expect("generate items module");

        let generated = std::fs::read_to_string(temp_dir.join("items.rs"))
            .expect("generated items.rs should exist");
        assert!(generated.contains("\"minecraft:honey_bottle\" => Some(&HONEY_BOTTLE)"));
        assert!(generated.contains("737i32 => Some(&HONEY_BOTTLE)"));
        assert!(!generated.contains("ALL_ITEMS.iter().find"));
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn rejects_duplicate_network_ids() {
        let items = vec![
            ParsedItem {
                identifier: "minecraft:a".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
                max_stack_size: 64,
                max_stack_size_source: "test".to_string(),
            },
            ParsedItem {
                identifier: "minecraft:b".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
                max_stack_size: 64,
                max_stack_size_source: "test".to_string(),
            },
        ];

        let error = validate_items(&items).expect_err("duplicate network ID is invalid");

        assert!(error.to_string().contains("duplicate item network_id"));
    }

    #[test]
    fn rejects_network_ids_outside_packet_range() {
        let items = vec![ParsedItem {
            identifier: "minecraft:a".to_string(),
            network_id: i16::MAX as i32 + 1,
            component_based: false,
            version: 2,
            max_stack_size: 64,
            max_stack_size_source: "test".to_string(),
        }];

        let error = validate_items(&items).expect_err("out-of-range network ID is invalid");

        assert!(error.to_string().contains("out of i16 packet range"));
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }
}
