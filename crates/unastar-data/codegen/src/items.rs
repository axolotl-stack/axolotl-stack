//! Item registry code generation from normalized KDL artifacts.

use heck::ToShoutySnakeCase;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    tags: Vec<String>,
    native_properties_json: String,
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
        .ok_or_else(|| miette::miette!("item {} missing max_stack_size", identifier))
        .and_then(|value| {
            u8::try_from(value).map_err(|_| {
                miette::miette!("item {} max_stack_size is out of u8 range", identifier)
            })
        })?;
    if max_stack_size == 0 {
        return Err(miette::miette!(
            "item {} max_stack_size must be greater than zero",
            identifier
        ));
    }

    let max_stack_size_source = node
        .get("max_stack_size_source")
        .and_then(|entry| entry.as_string())
        .ok_or_else(|| miette::miette!("item {} missing max_stack_size_source", identifier))?
        .to_string();

    let mut tags = BTreeSet::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let tag = child.get(0).and_then(|value| value.as_string());
            if child.name().value() != "tag"
                || child.entries().len() != 1
                || child.entries()[0].name().is_some()
                || child.children().is_some()
                || tag.is_none_or(|tag| tag.trim().is_empty())
            {
                return Err(miette::miette!(
                    "item {} has malformed native tag",
                    identifier
                ));
            }
            tags.insert(tag.expect("validated tag").to_string());
        }
    }

    let mut native_properties = BTreeMap::new();
    for entry in node.entries() {
        let Some(name) = entry.name() else { continue };
        let name = name.value();
        if matches!(
            name,
            "runtime_id"
                | "component_based"
                | "version"
                | "max_stack_size"
                | "max_stack_size_source"
                | "registry_metadata_source"
        ) {
            continue;
        }
        let value = entry.value();
        let json = if let Some(value) = value.as_string() {
            serde_json::Value::String(value.to_string())
        } else if let Some(value) = value.as_bool() {
            serde_json::Value::Bool(value)
        } else if let Some(value) = value.as_integer() {
            let value = i64::try_from(value).map_err(|_| {
                miette::miette!(
                    "item {} property {} exceeds JSON integer range",
                    identifier,
                    name
                )
            })?;
            serde_json::Value::Number(value.into())
        } else if let Some(value) = value.as_float() {
            serde_json::Value::Number(serde_json::Number::from_f64(value).ok_or_else(|| {
                miette::miette!(
                    "item {} property {} is not a finite JSON number",
                    identifier,
                    name
                )
            })?)
        } else {
            return Err(miette::miette!(
                "item {} property {} must be a native scalar",
                identifier,
                name
            ));
        };
        if native_properties.insert(name, json).is_some() {
            return Err(miette::miette!(
                "item {} has duplicate native property {}",
                identifier,
                name
            ));
        }
    }
    let native_properties_json = serde_json::to_string(&native_properties).map_err(|error| {
        miette::miette!(
            "item {} properties cannot be serialized: {}",
            identifier,
            error
        )
    })?;

    Ok(ParsedItem {
        identifier,
        network_id,
        component_based,
        version,
        max_stack_size,
        max_stack_size_source,
        tags: tags.into_iter().collect(),
        native_properties_json,
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
            let tags = &item.tags;
            let native_properties_json = &item.native_properties_json;
            quote! {
                pub const #const_name: ItemData = ItemData {
                    id: #id,
                    identifier: #identifier,
                    network_id: #network_id,
                    component_based: #component_based,
                    version: #version,
                    max_stack_size: #max_stack_size,
                    max_stack_size_source: #max_stack_size_source,
                    tags: &[#(#tags),*],
                    native_properties_json: #native_properties_json,
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
        //! Generated item registry data from normalized protocolgen artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! IDs, stack sizes and gameplay properties come from the native item
        //! table; packet registry metadata comes from protocolgen's items.nbt.

        /// Source-attributed item registry row.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ItemData {
            /// Internal registry ID assigned by deterministic codegen order.
            ///
            /// This is not a Bedrock packet/network item ID.
            pub id: u32,
            /// Namespaced item identifier.
            pub identifier: &'static str,
            /// Signed protocol network ID from the native item table.
            pub network_id: i32,
            /// Whether the item is component-based in Bedrock's item registry.
            pub component_based: bool,
            /// Bedrock item registry version discriminator.
            pub version: i32,
            /// Maximum stack size from the native item properties table.
            pub max_stack_size: u8,
            /// Field-level source for `max_stack_size`.
            pub max_stack_size_source: &'static str,
            /// Sorted, deduplicated tags from the native item table and tag index.
            pub tags: &'static [&'static str],
            /// Canonical JSON object containing additional native gameplay properties.
            ///
            /// Keys retain their native spelling (for example, `maxDamage`).
            /// Registry metadata and fields exposed above are excluded.
            pub native_properties_json: &'static str,
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
            r#"item minecraft:apple runtime_id=285 component_based=#true version=1 max_stack_size=64 max_stack_size_source=protocolgen_native"#
                .parse()
                .expect("parse item");

        let item = parse_item_node(&doc.nodes()[0]).expect("parse item node");

        assert_eq!(item.identifier, "minecraft:apple");
        assert_eq!(item.network_id, 285);
        assert!(item.component_based);
        assert_eq!(item.version, 1);
        assert_eq!(item.max_stack_size, 64);
        assert_eq!(item.max_stack_size_source, "protocolgen_native");
    }

    #[test]
    fn rejects_unsourced_or_missing_stack_size() {
        for input in [
            "item minecraft:apple runtime_id=285 component_based=#false version=2",
            "item minecraft:apple runtime_id=285 component_based=#false version=2 max_stack_size=64",
        ] {
            let doc: KdlDocument = input.parse().unwrap();
            assert!(parse_item_node(&doc.nodes()[0]).is_err());
        }
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
    fn preserves_native_properties_and_canonical_tags() {
        let doc: KdlDocument = r#"item minecraft:boat runtime_id=382 component_based=#false version=2 max_stack_size=1 max_stack_size_source=protocolgen_native registry_metadata_source=protocolgen_registry descriptionId="item.boat" maxDamage=0 fireResistant=#false furnaceBurnIntervalModifier=1.5 {
            tag minecraft:boats
            tag minecraft:boat
            tag minecraft:boats
        }"#.parse().unwrap();
        let item = parse_item_node(&doc.nodes()[0]).unwrap();
        assert_eq!(item.tags, ["minecraft:boat", "minecraft:boats"]);
        let properties: serde_json::Value =
            serde_json::from_str(&item.native_properties_json).unwrap();
        assert_eq!(
            properties,
            serde_json::json!({
                "descriptionId": "item.boat",
                "maxDamage": 0,
                "fireResistant": false,
                "furnaceBurnIntervalModifier": 1.5
            })
        );
    }

    #[test]
    fn rejects_malformed_native_tags() {
        for child in [
            "tag 12",
            "tag",
            "unknown minecraft:boat",
            "tag minecraft:boat extra",
        ] {
            let doc: KdlDocument = format!(
                "item minecraft:boat runtime_id=382 component_based=#false version=2 max_stack_size=1 max_stack_size_source=protocolgen_native {{ {child}; }}"
            ).parse().unwrap();
            assert!(parse_item_node(&doc.nodes()[0]).is_err(), "{child}");
        }
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
                tags: Vec::new(),
                native_properties_json: "{}".to_string(),
            },
            ParsedItem {
                identifier: "minecraft:b".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
                max_stack_size: 64,
                max_stack_size_source: "test".to_string(),
                tags: Vec::new(),
                native_properties_json: "{}".to_string(),
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
            tags: Vec::new(),
            native_properties_json: "{}".to_string(),
        }];

        let error = validate_items(&items).expect_err("out-of-range network ID is invalid");

        assert!(error.to_string().contains("out of i16 packet range"));
    }
}
