//! Item data generation from items.json.
//!
//! Generates ZST item types implementing ItemDef trait + extensions.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use tracing::debug;

/// Raw item entry from minecraft-data items.json.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemJson {
    id: u32,
    name: String,
    display_name: String,
    #[serde(default = "default_stack_size")]
    stack_size: u8,
    #[serde(default)]
    metadata: u32,
    #[serde(default)]
    max_durability: Option<u16>,
    #[serde(default)]
    repair_with: Vec<String>,
    #[serde(default)]
    enchant_categories: Vec<String>,
    #[serde(default)]
    variations: Vec<ItemVariantJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemVariantJson {
    id: u32,
    metadata: u32,
    name: String,
    display_name: String,
    stack_size: u8,
}

fn default_stack_size() -> u8 {
    64
}

/// Convert snake_case to PascalCase.
fn to_pascal_case(name: &str) -> String {
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

/// Parse enchantment category string to enum variant name.
fn parse_enchant_category(s: &str) -> Option<&'static str> {
    match s {
        "weapon" => Some("Weapon"),
        "sword" => Some("Sword"),
        "axe" => Some("Axe"),
        "pickaxe" => Some("Pickaxe"),
        "shovel" => Some("Shovel"),
        "hoe" => Some("Hoe"),
        "head_armor" => Some("HeadArmor"),
        "chest_armor" => Some("ChestArmor"),
        "legs_armor" => Some("LegsArmor"),
        "feet_armor" => Some("FeetArmor"),
        "armor" => Some("Armor"),
        "equippable" => Some("Equippable"),
        "bow" => Some("Bow"),
        "crossbow" => Some("Crossbow"),
        "trident" => Some("Trident"),
        "fishing_rod" => Some("FishingRod"),
        "shears" => Some("Shears"),
        "flint_and_steel" => Some("FlintAndSteel"),
        "shield" => Some("Shield"),
        "elytra" => Some("Elytra"),
        "durability" => Some("Durability"),
        "vanishing" => Some("Vanishing"),
        "mending" => Some("Mending"),
        _ => None,
    }
}

/// Generate items.rs from items.json.
pub fn generate_items(
    json_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(json_path)?;
    let items: Vec<ItemJson> = serde_json::from_reader(BufReader::new(file))?;

    debug!(count = items.len(), "Generating ZST items with traits");

    // Build name->ID map for repair item resolution
    let name_to_id: HashMap<String, u32> = items
        .iter()
        .map(|item| (item.name.clone(), item.id))
        .collect();

    // Build set of clean names to detect collisions
    // First pass: collect all clean names from non-"item." items
    let base_names: std::collections::HashSet<String> = items
        .iter()
        .filter(|item| !item.name.starts_with("item."))
        .map(|item| to_pascal_case(&item.name))
        .collect();

    let output_path = output_dir.join("items.rs");
    let mut out = File::create(&output_path)?;

    // Header
    writeln!(out, "//! Generated vanilla item definitions.")?;
    writeln!(out, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(out)?;
    writeln!(
        out,
        "use valentine_bedrock_core::item::{{ItemDef, ItemDefDyn, DurableItem, RepairableItem, EnchantableItem, VariantItem, EnchantmentCategory, ItemVariant}};"
    )?;
    writeln!(out)?;

    // Track used struct names to avoid duplicates
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Generate ZST for each item
    for item in &items {
        // Sanitize name: strip "item." prefix if present
        let clean_name = if item.name.starts_with("item.") {
            &item.name[5..]
        } else {
            &item.name
        };
        let mut struct_name = to_pascal_case(clean_name);

        // If this is an "item." prefixed item and the name collides with a base item,
        // append "Item" suffix to distinguish it
        if item.name.starts_with("item.") && base_names.contains(&struct_name) {
            struct_name = format!("{}Item", struct_name);
        }

        // Skip if we've already generated this struct (shouldn't happen but safety check)
        if used_names.contains(&struct_name) {
            tracing::warn!(name = %struct_name, original = %item.name, "Skipping duplicate item");
            continue;
        }
        used_names.insert(struct_name.clone());

        let display_name = item.display_name.replace('"', r#"\""#);

        writeln!(out, "/// {}", item.display_name)?;
        writeln!(out, "pub struct {};", struct_name)?;
        writeln!(out)?;

        // Generate ItemDef impl
        writeln!(out, "impl ItemDef for {} {{", struct_name)?;
        writeln!(out, "    const ID: u32 = {};", item.id)?;
        writeln!(
            out,
            r#"    const STRING_ID: &'static str = "minecraft:{}";"#,
            item.name
        )?;
        writeln!(out, r#"    const NAME: &'static str = "{}";"#, display_name)?;
        writeln!(out, "    const STACK_SIZE: u8 = {};", item.stack_size)?;
        if item.metadata != 0 {
            writeln!(out, "    const METADATA: u32 = {};", item.metadata)?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;

        // Generate DurableItem impl if needed
        if let Some(durability) = item.max_durability {
            writeln!(out, "impl DurableItem for {} {{", struct_name)?;
            writeln!(out, "    const MAX_DURABILITY: u16 = {};", durability)?;
            writeln!(out, "}}")?;
            writeln!(out)?;
        }

        // Generate RepairableItem impl if needed
        if !item.repair_with.is_empty() {
            let repair_ids: Vec<u32> = item
                .repair_with
                .iter()
                .filter_map(|name| name_to_id.get(name).copied())
                .collect();

            if !repair_ids.is_empty() {
                writeln!(out, "impl RepairableItem for {} {{", struct_name)?;
                writeln!(out, "    fn repair_items() -> &'static [u32] {{")?;
                writeln!(out, "        &{:?}", repair_ids)?;
                writeln!(out, "    }}")?;
                writeln!(out, "}}")?;
                writeln!(out)?;
            }
        }

        // Generate EnchantableItem impl if needed
        if !item.enchant_categories.is_empty() {
            let categories: Vec<&str> = item
                .enchant_categories
                .iter()
                .filter_map(|s| parse_enchant_category(s))
                .collect();

            if !categories.is_empty() {
                writeln!(out, "impl EnchantableItem for {} {{", struct_name)?;
                writeln!(
                    out,
                    "    fn enchant_categories() -> &'static [EnchantmentCategory] {{"
                )?;
                write!(out, "        &[")?;
                for (i, cat) in categories.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "EnchantmentCategory::{}", cat)?;
                }
                writeln!(out, "]")?;
                writeln!(out, "    }}")?;
                writeln!(out, "}}")?;
                writeln!(out)?;
            }
        }

        // Generate VariantItem impl if needed
        if !item.variations.is_empty() {
            writeln!(out, "impl VariantItem for {} {{", struct_name)?;
            writeln!(out, "    fn variants() -> &'static [ItemVariant] {{")?;
            writeln!(out, "        &[")?;
            for variant in &item.variations {
                let variant_display = variant.display_name.replace('"', r#"\""#);
                writeln!(out, "            ItemVariant {{")?;
                writeln!(out, "                id: {},", variant.id)?;
                writeln!(out, "                metadata: {},", variant.metadata)?;
                writeln!(out, r#"                name: "{}","#, variant.name)?;
                writeln!(
                    out,
                    r#"                display_name: "{}","#,
                    variant_display
                )?;
                writeln!(out, "                stack_size: {},", variant.stack_size)?;
                writeln!(out, "            }},")?;
            }
            writeln!(out, "        ]")?;
            writeln!(out, "    }}")?;
            writeln!(out, "}}")?;
            writeln!(out)?;
        }
    }

    // Generate registry array
    writeln!(out, "/// All vanilla items as dynamic references.")?;
    writeln!(out, "pub static ITEMS: &[&'static dyn ItemDefDyn] = &[")?;

    // Rebuild used names set for proper struct name references
    let mut registry_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for item in &items {
        let clean_name = if item.name.starts_with("item.") {
            &item.name[5..]
        } else {
            &item.name
        };
        let mut struct_name = to_pascal_case(clean_name);

        // Same collision logic as struct generation
        if item.name.starts_with("item.") && base_names.contains(&struct_name) {
            struct_name = format!("{}Item", struct_name);
        }

        // Skip duplicates (same as struct generation)
        if registry_names.contains(&struct_name) {
            continue;
        }
        registry_names.insert(struct_name.clone());

        writeln!(out, "    &{},", struct_name)?;
    }
    writeln!(out, "];")?;

    Ok(())
}
