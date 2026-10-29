//! Item registry code generation from normalized KDL artifacts.

use base64::Engine;
use heck::ToSnakeCase;
use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use tracing::info;

/// The unnamed empty network NBT compound (`0a 00 00`).
const EMPTY_COMPONENT_NBT: &[u8] = &[0x0a, 0x00, 0x00];

/// One native item property value, typed as in `items.kdl`.
#[derive(Debug, Clone, PartialEq)]
enum NativeValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

impl NativeValue {
    fn kind(&self) -> NativeKind {
        match self {
            Self::Bool(_) => NativeKind::Bool,
            Self::Integer(_) => NativeKind::Integer,
            Self::Float(_) => NativeKind::Float,
            Self::String(_) => NativeKind::String,
        }
    }

    fn to_json(&self, kind: NativeKind) -> serde_json::Value {
        match (self, kind) {
            (Self::Bool(value), _) => serde_json::Value::Bool(*value),
            (Self::Integer(value), NativeKind::Float) => serde_json::json!(*value as f64),
            (Self::Integer(value), _) => serde_json::json!(*value),
            (Self::Float(value), _) => serde_json::json!(*value),
            (Self::String(value), _) => serde_json::Value::String(value.clone()),
        }
    }
}

/// Rust type of a native property column: the widest kind any item uses
/// (an integer column with one fractional value becomes `f32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NativeKind {
    Bool,
    Integer,
    Float,
    String,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedItem {
    identifier: String,
    network_id: i32,
    component_based: bool,
    version: i32,
    max_stack_size: u8,
    max_stack_size_source: String,
    tags: Vec<String>,
    /// Inventory capability names (`minecraft:allow_offhand`, `minecraft:head`).
    capabilities: Vec<String>,
    /// Native gameplay properties keyed by their native spelling.
    native_properties: BTreeMap<String, NativeValue>,
    /// Exact network NBT of the item's ItemRegistryPacket component data.
    component_nbt: Vec<u8>,
}

pub fn generate_items(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let path = input_dir.join("items.kdl");
    if !path.exists() {
        return Err(miette::miette!("items.kdl not found at {}", path.display()));
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

    items.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    validate_items(&items)?;
    let schema = ItemSchema::infer(&items)?;

    info!("Parsed {} item registry rows", items.len());

    std::fs::write(output_dir.join("items.rs"), generate_items_module(&schema)?)
        .map_err(|e| miette::miette!("Failed to write items.rs: {}", e))?;
    std::fs::write(
        output_dir.join(ITEMS_JSON),
        generate_items_json(&items, &schema)?,
    )
    .map_err(|e| miette::miette!("Failed to write {ITEMS_JSON}: {}", e))?;
    Ok(())
}

/// File name of the embedded item rows next to the generated schema.
const ITEMS_JSON: &str = "items.json";

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

    // Absent means BDS sends the unnamed empty compound.
    let component_nbt = match node.get("component_nbt") {
        None => EMPTY_COMPONENT_NBT.to_vec(),
        Some(value) => {
            let encoded = value.as_string().ok_or_else(|| {
                miette::miette!("item {} component_nbt must be a string", identifier)
            })?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|e| {
                    miette::miette!("item {} component_nbt is not base64: {}", identifier, e)
                })?;
            if !bytes.starts_with(&[0x0a, 0x00]) || bytes.last() != Some(&0) {
                return Err(miette::miette!(
                    "item {} component_nbt is not an unnamed network NBT compound",
                    identifier
                ));
            }
            bytes
        }
    };
    let mut tags = BTreeSet::new();
    let mut capabilities = BTreeSet::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            let value = child.get(0).and_then(|value| value.as_string());
            let kind = child.name().value();
            let well_formed = child.entries().first().is_some_and(|e| e.name().is_none())
                && child.children().is_none()
                && value.is_some_and(|value| !value.trim().is_empty());
            match kind {
                "tag" if well_formed && child.entries().len() == 1 => {
                    tags.insert(value.expect("validated tag").to_string());
                }
                "capability"
                    if well_formed
                        && child.entries().len() == 2
                        && child.get("source").and_then(|s| s.as_string()).is_some() =>
                {
                    capabilities.insert(value.expect("validated capability").to_string());
                }
                _ => {
                    return Err(miette::miette!(
                        "item {} has malformed {} child",
                        identifier,
                        kind
                    ));
                }
            }
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
                | "component_nbt"
        ) {
            continue;
        }
        let value = entry.value();
        let native = if let Some(value) = value.as_string() {
            NativeValue::String(value.to_string())
        } else if let Some(value) = value.as_bool() {
            NativeValue::Bool(value)
        } else if let Some(value) = value.as_integer() {
            NativeValue::Integer(i64::try_from(value).map_err(|_| {
                miette::miette!(
                    "item {} property {} exceeds the integer range",
                    identifier,
                    name
                )
            })?)
        } else if let Some(value) = value.as_float().filter(|value| value.is_finite()) {
            NativeValue::Float(value)
        } else {
            return Err(miette::miette!(
                "item {} property {} must be a finite native scalar",
                identifier,
                name
            ));
        };
        if native_properties.insert(name.to_string(), native).is_some() {
            return Err(miette::miette!(
                "item {} has duplicate native property {}",
                identifier,
                name
            ));
        }
    }

    Ok(ParsedItem {
        identifier,
        network_id,
        component_based,
        version,
        max_stack_size,
        max_stack_size_source,
        tags: tags.into_iter().collect(),
        capabilities: capabilities.into_iter().collect(),
        native_properties,
        component_nbt,
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

/// The typed schema inferred from every row: one column per native property
/// (all items must carry all of them) and one flag per inventory capability.
#[derive(Debug, Clone, PartialEq)]
struct ItemSchema {
    /// Native spelling → column kind, in native key order.
    native: BTreeMap<String, NativeKind>,
    capabilities: BTreeSet<String>,
}

impl ItemSchema {
    fn infer(items: &[ParsedItem]) -> miette::Result<Self> {
        let mut native: BTreeMap<String, NativeKind> = BTreeMap::new();
        for item in items {
            for (key, value) in &item.native_properties {
                let kind = value.kind();
                let column = native.entry(key.clone()).or_insert(kind);
                *column = match (*column, kind) {
                    (a, b) if a == b => a,
                    (NativeKind::Integer, NativeKind::Float)
                    | (NativeKind::Float, NativeKind::Integer) => NativeKind::Float,
                    (a, b) => {
                        return Err(miette::miette!(
                            "native item property {key} mixes {a:?} and {b:?} values"
                        ));
                    }
                };
            }
        }
        for item in items {
            if let Some(missing) = native
                .keys()
                .find(|key| !item.native_properties.contains_key(*key))
            {
                return Err(miette::miette!(
                    "item {} lacks native property {missing}",
                    item.identifier
                ));
            }
        }
        let capabilities = items
            .iter()
            .flat_map(|item| item.capabilities.iter().cloned())
            .collect();
        let schema = Self {
            native,
            capabilities,
        };
        let mut fields = HashSet::new();
        for field in FIXED_FIELDS
            .iter()
            .map(|field| field.to_string())
            .chain(schema.native.keys().map(|key| native_field_name(key)))
            .chain(
                schema
                    .capabilities
                    .iter()
                    .map(|name| capability_field_name(name)),
            )
        {
            if !fields.insert(field.clone()) {
                return Err(miette::miette!(
                    "item schema field {field} is defined twice"
                ));
            }
        }
        Ok(schema)
    }
}

/// Fields every row has, independent of the native column set.
const FIXED_FIELDS: [&str; 7] = [
    "identifier",
    "network_id",
    "component_based",
    "version",
    "max_stack_size",
    "tags",
    "component_nbt",
];

fn native_field_name(key: &str) -> String {
    key.to_snake_case()
}

fn capability_field_name(capability: &str) -> String {
    capability
        .strip_prefix("minecraft:")
        .unwrap_or(capability)
        .to_snake_case()
}

/// Rows as compact JSON. Defaults are left out (`tags: []`, the empty
/// component compound, false capabilities) to keep the embedded data small.
fn generate_items_json(items: &[ParsedItem], schema: &ItemSchema) -> miette::Result<String> {
    let rows: Vec<serde_json::Value> = items
        .iter()
        .map(|item| {
            let mut row = serde_json::Map::new();
            row.insert("identifier".into(), item.identifier.clone().into());
            row.insert("network_id".into(), item.network_id.into());
            row.insert("component_based".into(), item.component_based.into());
            row.insert("version".into(), item.version.into());
            row.insert("max_stack_size".into(), item.max_stack_size.into());
            if !item.tags.is_empty() {
                row.insert("tags".into(), item.tags.clone().into());
            }
            for capability in &item.capabilities {
                row.insert(capability_field_name(capability), true.into());
            }
            if item.component_nbt != EMPTY_COMPONENT_NBT {
                row.insert(
                    "component_nbt".into(),
                    base64::engine::general_purpose::STANDARD
                        .encode(&item.component_nbt)
                        .into(),
                );
            }
            for (key, kind) in &schema.native {
                row.insert(
                    native_field_name(key),
                    item.native_properties[key].to_json(*kind),
                );
            }
            serde_json::Value::Object(row)
        })
        .collect();
    let mut json = serde_json::to_string(&rows)
        .map_err(|e| miette::miette!("Failed to serialize item rows: {}", e))?;
    json.push('\n');
    Ok(json)
}

fn generate_items_module(schema: &ItemSchema) -> miette::Result<String> {
    let native_fields: Vec<TokenStream> = schema
        .native
        .iter()
        .map(|(key, kind)| {
            let field = format_ident!("{}", native_field_name(key));
            let doc = format!(" Native `{key}`.");
            let ty = match kind {
                NativeKind::Bool => quote! { bool },
                NativeKind::Integer => quote! { i32 },
                NativeKind::Float => quote! { f32 },
                NativeKind::String => quote! { String },
            };
            quote! {
                #[doc = #doc]
                pub #field: #ty,
            }
        })
        .collect();
    let capability_fields: Vec<TokenStream> = schema
        .capabilities
        .iter()
        .map(|capability| {
            let field = format_ident!("{}", capability_field_name(capability));
            let doc = format!(
                " Inventory capability `{capability}` (protocolgen capability table and corrections)."
            );
            quote! {
                #[doc = #doc]
                #[serde(default)]
                pub #field: bool,
            }
        })
        .collect();

    let code = quote! {
        //! Item data schema generated from normalized protocolgen artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.
        //!
        //! The rows themselves live in `items.json` next to this file and are
        //! decoded once, on first use (D2: schemas are generated, data is
        //! embedded and decoded at startup). IDs, stack sizes and gameplay
        //! properties come from the native item table with protocolgen's
        //! reviewed corrections; packet registry metadata and component NBT come
        //! from protocolgen's items.nbt, which matches BDS 1.26.51's
        //! ItemRegistryPacket.

        use std::collections::HashMap;
        use std::sync::LazyLock;

        /// The unnamed empty network NBT compound BDS sends for items without
        /// registry component data.
        pub const EMPTY_COMPONENT_NBT: &[u8] = b"\x0a\x00\x00";

        /// One vanilla item row.
        #[derive(Debug, Clone, PartialEq, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct ItemData {
            /// Namespaced item identifier.
            pub identifier: String,
            /// Signed protocol network ID from the native item table.
            pub network_id: i32,
            /// Whether the item is component-based in Bedrock's item registry.
            pub component_based: bool,
            /// Bedrock item registry version discriminator.
            pub version: i32,
            /// Effective maximum stack size: the native table value, or the
            /// reviewed item-class correction where the native value is a base
            /// value BDS overrides (buckets).
            pub max_stack_size: u8,
            /// Sorted, deduplicated tags from the native item table and tag index.
            #[serde(default)]
            pub tags: Vec<String>,
            /// `ItemRegistryPacket` component data as exact network NBT: an unnamed
            /// root compound, `{}` for most items, and the component compound BDS
            /// sends for component-based and legacy-JSON items.
            #[serde(default = "empty_component_nbt", deserialize_with = "base64_bytes")]
            pub component_nbt: Vec<u8>,
            #(#capability_fields)*
            #(#native_fields)*
        }

        impl ItemData {
            /// Whether the item carries `tag`.
            pub fn has_tag(&self, tag: &str) -> bool {
                self.tags.binary_search_by(|t| t.as_str().cmp(tag)).is_ok()
            }
        }

        fn empty_component_nbt() -> Vec<u8> {
            EMPTY_COMPONENT_NBT.to_vec()
        }

        fn base64_bytes<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
            use base64::Engine as _;
            let encoded = <String as serde::Deserialize>::deserialize(deserializer)?;
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(serde::de::Error::custom)
        }

        /// Embedded vanilla rows, sorted by identifier.
        const VANILLA_ITEMS_JSON: &str = include_str!("items.json");

        /// Decode the embedded vanilla rows.
        pub fn decode_vanilla() -> Result<Vec<ItemData>, serde_json::Error> {
            serde_json::from_str(VANILLA_ITEMS_JSON)
        }

        struct Vanilla {
            rows: Vec<ItemData>,
            by_identifier: HashMap<String, usize>,
            by_network_id: HashMap<i32, usize>,
        }

        static VANILLA: LazyLock<Vanilla> = LazyLock::new(|| {
            let rows = decode_vanilla().expect("embedded vanilla item rows decode");
            let by_identifier = rows
                .iter()
                .enumerate()
                .map(|(index, row)| (row.identifier.clone(), index))
                .collect();
            let by_network_id = rows
                .iter()
                .enumerate()
                .map(|(index, row)| (row.network_id, index))
                .collect();
            Vanilla { rows, by_identifier, by_network_id }
        });

        /// All vanilla item rows sorted by identifier, decoded on first use.
        pub fn vanilla() -> &'static [ItemData] {
            &VANILLA.rows
        }

        /// Look up a vanilla item by namespaced identifier.
        pub fn get(identifier: &str) -> Option<&'static ItemData> {
            VANILLA.by_identifier.get(identifier).map(|&index| &VANILLA.rows[index])
        }

        /// Look up a vanilla item by signed protocol network ID.
        pub fn by_network_id(network_id: i32) -> Option<&'static ItemData> {
            VANILLA.by_network_id.get(&network_id).map(|&index| &VANILLA.rows[index])
        }
    };

    format_code(code)
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
    fn infers_typed_native_columns_and_writes_compact_rows() {
        let doc: KdlDocument = r#"item minecraft:boat runtime_id=382 component_based=#false version=2 max_stack_size=1 max_stack_size_source=protocolgen_native registry_metadata_source=protocolgen_registry descriptionId="item.boat" maxDamage=0 fireResistant=#false furnaceBurnIntervalModifier=1.5 {
            tag minecraft:boats
            tag minecraft:boat
            tag minecraft:boats
            capability minecraft:allow_offhand source=test
        }
        item minecraft:stick runtime_id=1 component_based=#false version=2 max_stack_size=64 max_stack_size_source=protocolgen_native descriptionId="item.stick" maxDamage=3 fireResistant=#true furnaceBurnIntervalModifier=0"#.parse().unwrap();
        let items: Vec<ParsedItem> = doc
            .nodes()
            .iter()
            .map(|node| parse_item_node(node).unwrap())
            .collect();
        assert_eq!(items[0].tags, ["minecraft:boat", "minecraft:boats"]);
        let schema = ItemSchema::infer(&items).unwrap();
        assert_eq!(
            schema.native.into_iter().collect::<Vec<_>>(),
            [
                ("descriptionId".to_string(), NativeKind::String),
                ("fireResistant".to_string(), NativeKind::Bool),
                ("furnaceBurnIntervalModifier".to_string(), NativeKind::Float),
                ("maxDamage".to_string(), NativeKind::Integer),
            ]
        );
        let schema = ItemSchema::infer(&items).unwrap();
        let rows: serde_json::Value =
            serde_json::from_str(&generate_items_json(&items, &schema).unwrap()).unwrap();
        assert_eq!(
            rows,
            serde_json::json!([
                {
                    "identifier": "minecraft:boat", "network_id": 382, "component_based": false,
                    "version": 2, "max_stack_size": 1, "tags": ["minecraft:boat", "minecraft:boats"],
                    "allow_offhand": true, "description_id": "item.boat", "fire_resistant": false,
                    "furnace_burn_interval_modifier": 1.5, "max_damage": 0
                },
                {
                    "identifier": "minecraft:stick", "network_id": 1, "component_based": false,
                    "version": 2, "max_stack_size": 64, "description_id": "item.stick",
                    "fire_resistant": true, "furnace_burn_interval_modifier": 0.0, "max_damage": 3
                }
            ])
        );
    }

    #[test]
    fn schema_rejects_missing_or_conflicting_native_columns() {
        let parse = |input: &str| -> Vec<ParsedItem> {
            let doc: KdlDocument = input.parse().unwrap();
            doc.nodes()
                .iter()
                .map(|node| parse_item_node(node).unwrap())
                .collect()
        };
        let base = "component_based=#false version=2 max_stack_size=1 max_stack_size_source=t";
        let missing = parse(&format!(
            "item a runtime_id=1 {base} maxDamage=1\nitem b runtime_id=2 {base}"
        ));
        assert!(
            ItemSchema::infer(&missing)
                .unwrap_err()
                .to_string()
                .contains("lacks native property")
        );
        let mixed = parse(&format!(
            "item a runtime_id=1 {base} glint=#true\nitem b runtime_id=2 {base} glint=1"
        ));
        assert!(
            ItemSchema::infer(&mixed)
                .unwrap_err()
                .to_string()
                .contains("mixes")
        );
    }

    #[test]
    fn component_nbt_defaults_to_empty_compound_and_rejects_malformed_values() {
        let parse = |extra: &str| {
            let doc: KdlDocument = format!(
                "item minecraft:apple runtime_id=285 component_based=#true version=1 max_stack_size=64 max_stack_size_source=protocolgen_native {extra}"
            )
            .parse()
            .unwrap();
            parse_item_node(&doc.nodes()[0])
        };
        let item = parse("").unwrap();
        assert_eq!(item.component_nbt, EMPTY_COMPONENT_NBT);
        // {a: byte 7} as an unnamed root compound.
        let item = parse(r#"component_nbt="CgABAWEHAA==""#).unwrap();
        assert_eq!(
            item.component_nbt,
            [0x0a, 0x00, 0x01, 0x01, b'a', 0x07, 0x00]
        );
        assert!(item.native_properties.is_empty());
        for bad in [
            r#"component_nbt="!""#,
            r#"component_nbt="AQA=""#,
            "component_nbt=1",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_malformed_native_tags() {
        for child in [
            "tag 12",
            "tag",
            "unknown minecraft:boat",
            "tag minecraft:boat extra",
            "capability minecraft:head",
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
                capabilities: Vec::new(),
                native_properties: BTreeMap::new(),
                component_nbt: EMPTY_COMPONENT_NBT.to_vec(),
            },
            ParsedItem {
                identifier: "minecraft:b".to_string(),
                network_id: -1,
                component_based: false,
                version: 2,
                max_stack_size: 64,
                max_stack_size_source: "test".to_string(),
                tags: Vec::new(),
                capabilities: Vec::new(),
                native_properties: BTreeMap::new(),
                component_nbt: EMPTY_COMPONENT_NBT.to_vec(),
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
            capabilities: Vec::new(),
            native_properties: BTreeMap::new(),
            component_nbt: EMPTY_COMPONENT_NBT.to_vec(),
        }];

        let error = validate_items(&items).expect_err("out-of-range network ID is invalid");

        assert!(error.to_string().contains("out of i16 packet range"));
    }
}
