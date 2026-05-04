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

    Ok(ParsedItem {
        identifier,
        network_id,
        component_based,
        version,
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
            quote! {
                pub const #const_name: ItemData = ItemData {
                    id: #id,
                    identifier: #identifier,
                    network_id: #network_id,
                    component_based: #component_based,
                    version: #version,
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
    let count = all_refs.len();

    let code = quote! {
        //! Generated item registry data from normalized PMMP/BedrockData artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! Stack sizes are intentionally absent until sourced from behavior-pack
        //! item components or BDS/native data.

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
        }

        #(#entries)*

        /// All generated item registry rows sorted by identifier.
        pub static ALL_ITEMS: [ItemData; #count] = [
            #(#all_refs),*
        ];

        /// Look up an item by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static ItemData> {
            ALL_ITEMS.iter().find(|item| item.identifier == identifier)
        }

        /// Look up an item by signed protocol network ID.
        pub fn by_network_id(network_id: i32) -> Option<&'static ItemData> {
            ALL_ITEMS.iter().find(|item| item.network_id == network_id)
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
    }

    #[test]
    fn rejects_duplicate_network_ids() {
        let items = vec![
            ParsedItem {
                identifier: "minecraft:a".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
            },
            ParsedItem {
                identifier: "minecraft:b".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
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
        }];

        let error = validate_items(&items).expect_err("out-of-range network ID is invalid");

        assert!(error.to_string().contains("out of i16 packet range"));
    }
}
