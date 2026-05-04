//! Creative inventory code generation from normalized KDL artifacts.

use kdl::{KdlDocument, KdlNode};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::info;

const EXPECTED_TABS: [&str; 4] = ["construction", "nature", "equipment", "items"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCreativeTab {
    name: String,
    source_file: String,
    groups: Vec<ParsedCreativeGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCreativeGroup {
    group_name: String,
    index: u32,
    icon: Option<ParsedCreativeEntry>,
    items: Vec<ParsedCreativeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCreativeEntry {
    name: String,
    block_states: Option<String>,
    damage: i16,
    nbt: Option<String>,
}

pub fn generate_creative(input_dir: &Path, output_dir: &Path) -> miette::Result<()> {
    let path = input_dir.join("creative.kdl");
    if !path.exists() {
        info!("No creative.kdl found at {}, skipping", path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| miette::miette!("Failed to read {}: {}", path.display(), e))?;
    let doc: KdlDocument = content
        .parse()
        .map_err(|e| miette::miette!("creative.kdl parse error: {}", e))?;

    let mut tabs = Vec::new();
    for node in doc.nodes() {
        if node.name().value() == "creative_tab" {
            tabs.push(parse_tab_node(node)?);
        }
    }

    validate_tabs(&tabs)?;
    info!(
        "Parsed {} creative tabs, {} groups, {} entries",
        tabs.len(),
        tabs.iter().map(|tab| tab.groups.len()).sum::<usize>(),
        tabs.iter()
            .flat_map(|tab| &tab.groups)
            .map(|group| group.items.len())
            .sum::<usize>()
    );

    generate_creative_module(&tabs, output_dir)
}

fn parse_tab_node(node: &KdlNode) -> miette::Result<ParsedCreativeTab> {
    let name = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("creative_tab node missing name"))?
        .to_string();
    let source_file = property_string(node, "source_file")
        .ok_or_else(|| miette::miette!("creative_tab {} missing source_file", name))?;

    let mut groups = Vec::new();
    let children = node
        .children()
        .ok_or_else(|| miette::miette!("creative_tab {} missing groups", name))?;
    for group in children.nodes() {
        if group.name().value() == "group" {
            groups.push(parse_group_node(group)?);
        }
    }

    Ok(ParsedCreativeTab {
        name,
        source_file,
        groups,
    })
}

fn parse_group_node(node: &KdlNode) -> miette::Result<ParsedCreativeGroup> {
    let group_name = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("creative group node missing group name"))?
        .to_string();
    let index = property_u32(node, "index", &group_name)?;

    let mut icon = None;
    let mut items = Vec::new();
    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "icon" => {
                    if icon.is_some() {
                        return Err(miette::miette!(
                            "creative group {} has multiple icons",
                            group_name
                        ));
                    }
                    icon = Some(parse_entry_node(child)?);
                }
                "item" => items.push(parse_entry_node(child)?),
                other => {
                    return Err(miette::miette!(
                        "creative group {} has unexpected child node {}",
                        group_name,
                        other
                    ));
                }
            }
        }
    }

    Ok(ParsedCreativeGroup {
        group_name,
        index,
        icon,
        items,
    })
}

fn parse_entry_node(node: &KdlNode) -> miette::Result<ParsedCreativeEntry> {
    let name = node
        .entries()
        .first()
        .and_then(|entry| entry.value().as_string())
        .ok_or_else(|| miette::miette!("creative {} node missing item name", node.name().value()))?
        .to_string();
    let block_states = property_string(node, "block_states");
    let nbt = property_string(node, "nbt");
    let meta = property_i16_optional(node, "meta", &name)?;
    let damage = property_i16_optional(node, "damage", &name)?;
    let damage = match (meta, damage) {
        (Some(meta), Some(damage)) if meta != damage => {
            return Err(miette::miette!(
                "creative item {} has conflicting meta {} and damage {}",
                name,
                meta,
                damage
            ));
        }
        (Some(meta), _) => meta,
        (_, Some(damage)) => damage,
        (None, None) => 0,
    };

    Ok(ParsedCreativeEntry {
        name,
        block_states,
        damage,
        nbt,
    })
}

fn property_string(node: &KdlNode, name: &str) -> Option<String> {
    node.get(name)
        .and_then(|entry| entry.as_string())
        .map(ToOwned::to_owned)
}

fn property_u32(node: &KdlNode, name: &str, context: &str) -> miette::Result<u32> {
    let value = node
        .get(name)
        .and_then(|entry| entry.as_integer())
        .ok_or_else(|| miette::miette!("{} missing {}", context, name))?;
    u32::try_from(value).map_err(|_| {
        miette::miette!(
            "{} property {} value {} is out of u32 range",
            context,
            name,
            value
        )
    })
}

fn property_i16_optional(node: &KdlNode, name: &str, context: &str) -> miette::Result<Option<i16>> {
    let Some(value) = node.get(name).and_then(|entry| entry.as_integer()) else {
        return Ok(None);
    };
    i16::try_from(value).map(Some).map_err(|_| {
        miette::miette!(
            "{} property {} value {} is out of i16 range",
            context,
            name,
            value
        )
    })
}

fn validate_tabs(tabs: &[ParsedCreativeTab]) -> miette::Result<()> {
    if tabs.is_empty() {
        return Err(miette::miette!(
            "creative.kdl did not contain any creative_tab nodes"
        ));
    }

    let mut seen_tabs = HashSet::new();
    for tab in tabs {
        if !EXPECTED_TABS.contains(&tab.name.as_str()) {
            return Err(miette::miette!("unexpected creative tab {}", tab.name));
        }
        if !seen_tabs.insert(tab.name.as_str()) {
            return Err(miette::miette!("duplicate creative tab {}", tab.name));
        }
        if tab.source_file.trim().is_empty() {
            return Err(miette::miette!(
                "creative tab {} source_file must be non-empty",
                tab.name
            ));
        }
        if tab.groups.is_empty() {
            return Err(miette::miette!(
                "creative tab {} did not contain any groups",
                tab.name
            ));
        }

        for (expected_index, group) in tab.groups.iter().enumerate() {
            if group.index != expected_index as u32 {
                return Err(miette::miette!(
                    "creative tab {} group {} has index {}, expected {}",
                    tab.name,
                    group.group_name,
                    group.index,
                    expected_index
                ));
            }
            if group.items.is_empty() {
                return Err(miette::miette!(
                    "creative tab {} group {} has no items",
                    tab.name,
                    group.group_name
                ));
            }
            if let Some(icon) = &group.icon {
                validate_entry(icon, &tab.name, &group.group_name)?;
            }
            for entry in &group.items {
                validate_entry(entry, &tab.name, &group.group_name)?;
            }
        }
    }

    for expected in EXPECTED_TABS {
        if !seen_tabs.contains(expected) {
            return Err(miette::miette!("missing creative tab {}", expected));
        }
    }

    Ok(())
}

fn validate_entry(entry: &ParsedCreativeEntry, tab: &str, group: &str) -> miette::Result<()> {
    if entry.name.trim().is_empty() {
        return Err(miette::miette!(
            "creative tab {} group {} contains empty item name",
            tab,
            group
        ));
    }
    if !entry.name.contains(':') {
        return Err(miette::miette!(
            "creative tab {} group {} item {} is not namespaced",
            tab,
            group,
            entry.name
        ));
    }
    Ok(())
}

fn generate_creative_module(tabs: &[ParsedCreativeTab], output_dir: &Path) -> miette::Result<()> {
    let tabs_by_name: HashMap<&str, &ParsedCreativeTab> =
        tabs.iter().map(|tab| (tab.name.as_str(), tab)).collect();

    let construction = tab_groups_tokens(tabs_by_name["construction"]);
    let nature = tab_groups_tokens(tabs_by_name["nature"]);
    let equipment = tab_groups_tokens(tabs_by_name["equipment"]);
    let items = tab_groups_tokens(tabs_by_name["items"]);

    let code = quote! {
        //! Generated creative inventory data from normalized PMMP/BedrockData artifacts.
        //!
        //! This module is auto-generated by `unastar-data-codegen`.
        //! Do not edit manually.

        /// Source family for all rows in this generated module.
        pub const SOURCE: &str = "pmmp_bedrock_data";

        /// Generated creative inventory entry.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct CreativeEntryData {
            /// Namespaced item identifier.
            pub name: &'static str,
            /// Base64-encoded block state NBT for block variants.
            pub block_states: Option<&'static str>,
            /// Legacy damage/meta variant value.
            pub damage: i16,
            /// Base64-encoded item NBT payload, when present.
            pub nbt: Option<&'static str>,
        }

        impl CreativeEntryData {
            /// Get the string ID of this creative item.
            pub fn item_id(&self) -> &'static str {
                self.name
            }

            /// Get base64-encoded block states if present.
            pub fn block_states(&self) -> Option<&'static str> {
                self.block_states
            }

            /// Get the legacy damage/meta variant value.
            pub fn damage(&self) -> i16 {
                self.damage
            }
        }

        /// Generated creative inventory group.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct CreativeGroupData {
            /// Source tab name from the normalized artifact.
            pub tab: &'static str,
            /// Source JSON file for this tab.
            pub source_file: &'static str,
            /// Localized creative group name.
            pub group_name: &'static str,
            /// Group index within its source tab.
            pub index: u32,
            /// Icon item for this group, when present.
            pub group_icon: Option<CreativeEntryData>,
            /// Creative items in source order.
            pub items: &'static [CreativeEntryData],
        }

        /// Generated creative inventory grouped by Bedrock creative tabs.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct CreativeInventoryData {
            pub construction: &'static [CreativeGroupData],
            pub nature: &'static [CreativeGroupData],
            pub equipment: &'static [CreativeGroupData],
            pub items: &'static [CreativeGroupData],
        }

        impl CreativeInventoryData {
            /// Load the generated creative inventory table.
            pub fn load() -> &'static Self {
                &CREATIVE_INVENTORY
            }

            /// Get all groups in vanilla Bedrock creative tab order.
            pub fn all_groups_ordered(&self) -> [(&'static str, &'static [CreativeGroupData]); 4] {
                [
                    ("Construction", self.construction),
                    ("Nature", self.nature),
                    ("Equipment", self.equipment),
                    ("Items", self.items),
                ]
            }
        }

        /// All generated creative inventory data.
        pub static CREATIVE_INVENTORY: CreativeInventoryData = CreativeInventoryData {
            construction: #construction,
            nature: #nature,
            equipment: #equipment,
            items: #items,
        };
    };

    std::fs::write(output_dir.join("creative.rs"), format_code(code)?)
        .map_err(|e| miette::miette!("Failed to write creative.rs: {}", e))?;

    Ok(())
}

fn tab_groups_tokens(tab: &ParsedCreativeTab) -> TokenStream {
    let tab_name = &tab.name;
    let source_file = &tab.source_file;
    let groups: Vec<_> = tab
        .groups
        .iter()
        .map(|group| {
            let group_name = &group.group_name;
            let index = group.index;
            let icon = group
                .icon
                .as_ref()
                .map(|entry| {
                    let entry = entry_tokens(entry);
                    quote! { Some(#entry) }
                })
                .unwrap_or_else(|| quote! { None });
            let items: Vec<_> = group.items.iter().map(entry_tokens).collect();
            quote! {
                CreativeGroupData {
                    tab: #tab_name,
                    source_file: #source_file,
                    group_name: #group_name,
                    index: #index,
                    group_icon: #icon,
                    items: &[#(#items),*],
                }
            }
        })
        .collect();

    quote! { &[#(#groups),*] }
}

fn entry_tokens(entry: &ParsedCreativeEntry) -> TokenStream {
    let name = &entry.name;
    let block_states = option_str_tokens(entry.block_states.as_deref());
    let damage = entry.damage;
    let nbt = option_str_tokens(entry.nbt.as_deref());
    quote! {
        CreativeEntryData {
            name: #name,
            block_states: #block_states,
            damage: #damage,
            nbt: #nbt,
        }
    }
}

fn option_str_tokens(value: Option<&str>) -> TokenStream {
    match value {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    }
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
    fn parses_simple_and_complex_entries() {
        let simple: KdlDocument = r#"item "minecraft:stone""#.parse().expect("parse simple");
        let complex: KdlDocument = r#"item "minecraft:arrow" meta=6 block_states="abc" nbt="def""#
            .parse()
            .expect("parse complex");

        let simple = parse_entry_node(&simple.nodes()[0]).expect("parse simple entry");
        let complex = parse_entry_node(&complex.nodes()[0]).expect("parse complex entry");

        assert_eq!(simple.name, "minecraft:stone");
        assert_eq!(simple.damage, 0);
        assert_eq!(complex.name, "minecraft:arrow");
        assert_eq!(complex.damage, 6);
        assert_eq!(complex.block_states.as_deref(), Some("abc"));
        assert_eq!(complex.nbt.as_deref(), Some("def"));
    }

    #[test]
    fn rejects_conflicting_meta_and_damage() {
        let doc: KdlDocument = r#"item "minecraft:arrow" meta=6 damage=7"#
            .parse()
            .expect("parse item");

        let error = parse_entry_node(&doc.nodes()[0]).expect_err("conflicting variants reject");

        assert!(error.to_string().contains("conflicting meta"));
    }

    #[test]
    fn rejects_missing_tabs() {
        let tabs = vec![tab("construction", 1)];

        let error = validate_tabs(&tabs).expect_err("missing tabs reject");

        assert!(error.to_string().contains("missing creative tab"));
    }

    #[test]
    fn rejects_non_contiguous_group_indices() {
        let mut tabs = complete_tabs();
        tabs[0].groups[0].index = 1;

        let error = validate_tabs(&tabs).expect_err("bad group index rejects");

        assert!(error.to_string().contains("expected 0"));
    }

    #[test]
    fn accepts_complete_tabs() {
        validate_tabs(&complete_tabs()).expect("complete tabs validate");
    }

    fn complete_tabs() -> Vec<ParsedCreativeTab> {
        EXPECTED_TABS.into_iter().map(|name| tab(name, 1)).collect()
    }

    fn tab(name: &str, groups: usize) -> ParsedCreativeTab {
        ParsedCreativeTab {
            name: name.to_string(),
            source_file: format!("creative_{name}.json"),
            groups: (0..groups)
                .map(|index| ParsedCreativeGroup {
                    group_name: format!("group_{index}"),
                    index: index as u32,
                    icon: None,
                    items: vec![ParsedCreativeEntry {
                        name: "minecraft:stone".to_string(),
                        block_states: None,
                        damage: 0,
                        nbt: None,
                    }],
                })
                .collect(),
        }
    }
}
