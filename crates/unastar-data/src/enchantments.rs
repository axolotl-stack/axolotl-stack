//! Enchantment table for the anvil, grindstone and enchanting table
//! (`data/measured/enchantments.json`, public reference, see its `source`),
//! decoded once.
//!
//! Each enchantment lists the item categories that take it; a category is an
//! armour slot, item tags, item identifiers or "any item with durability".
//! The enchanting table adds the option list each enchantment is sent in.
//! Random enchanting (weights, levels, enchantability) is
//! `unastar_rules::enchanting`.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Which items a category covers. Any matching field is enough, unless the
/// item is excluded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
    /// Player armour slot (0 head, 1 chest, 2 legs, 3 feet) the item is worn in.
    #[serde(default)]
    pub armor_slot: Option<u8>,
    /// Native item tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Item identifiers.
    #[serde(default)]
    pub items: Vec<String>,
    /// Every item with durability.
    #[serde(default)]
    pub durable: bool,
    /// Item identifiers the category never covers, whatever else matches.
    #[serde(default)]
    pub exclude_items: Vec<String>,
}

/// The `PlayerEnchantOptions` list an enchantment is sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    Equipped,
    Held,
    #[serde(rename = "self")]
    SelfActivated,
}

impl Activation {
    /// Index of the list in an option's three enchantment lists.
    pub fn index(self) -> usize {
        match self {
            Self::Equipped => 0,
            Self::Held => 1,
            Self::SelfActivated => 2,
        }
    }
}

/// One enchantment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enchantment {
    /// Bedrock enchantment id (the `id` of an `ench` entry).
    pub id: i16,
    pub name: String,
    pub max_level: i16,
    /// Anvil cost per level when the sacrifice is an item.
    pub item_multiplier: i32,
    /// Anvil cost per level when the sacrifice is an enchanted book.
    pub book_multiplier: i32,
    /// Minimum enchanting level of level 1.
    pub min_cost_base: i32,
    /// Minimum enchanting level added per level above 1.
    pub min_cost_per_level: i32,
    /// Curses survive the grindstone.
    pub curse: bool,
    /// Category names (keys of [`categories`]) of the items that take it.
    pub targets: Vec<String>,
    /// The enchanting-table option list that carries it.
    pub activation: Activation,
}

impl Enchantment {
    /// Minimum enchanting level of `level`.
    pub fn min_cost(&self, level: i16) -> i32 {
        self.min_cost_base + (i32::from(level) - 1) * self.min_cost_per_level
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    categories: HashMap<String, Category>,
    enchantments: Vec<Enchantment>,
    incompatible: Vec<Vec<i16>>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/enchantments.json"))
        .expect("embedded enchantment table decodes")
});

static BY_ID: LazyLock<HashMap<i16, &'static Enchantment>> =
    LazyLock::new(|| TABLE.enchantments.iter().map(|e| (e.id, e)).collect());

/// Where the table comes from.
pub fn source() -> &'static str {
    &TABLE.source
}

/// Every enchantment in id order.
pub fn all() -> &'static [Enchantment] {
    &TABLE.enchantments
}

/// The enchantment with Bedrock id `id`.
pub fn get(id: i16) -> Option<&'static Enchantment> {
    BY_ID.get(&id).copied()
}

/// The category called `name`.
pub fn category(name: &str) -> Option<&'static Category> {
    TABLE.categories.get(name)
}

/// Whether two different enchantments exclude each other.
pub fn incompatible(a: i16, b: i16) -> bool {
    a != b
        && TABLE
            .incompatible
            .iter()
            .any(|group| group.contains(&a) && group.contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_names_a_category_and_ids_are_unique() {
        assert!(source().starts_with("public reference"));
        assert_eq!(BY_ID.len(), all().len());
        for enchantment in all() {
            for target in &enchantment.targets {
                assert!(category(target).is_some(), "{}: {target}", enchantment.name);
            }
            assert!(enchantment.max_level >= 1);
        }
        for group in &TABLE.incompatible {
            for id in group {
                assert!(get(*id).is_some(), "{id}");
            }
        }
    }

    #[test]
    fn groups_exclude_their_members_only() {
        // Sharpness/smite, density/bane; riptide/loyalty and riptide/channeling
        // but not loyalty/channeling.
        assert!(incompatible(9, 10));
        assert!(incompatible(39, 11));
        assert!(!incompatible(9, 39));
        assert!(incompatible(30, 31) && incompatible(30, 32));
        assert!(!incompatible(31, 32));
        assert!(!incompatible(9, 9));
        // Sharpness III: minimum level 23 (the grindstone's 12 to 23 XP).
        assert_eq!(get(9).unwrap().min_cost(3), 23);
    }

    #[test]
    fn activation_lists() {
        assert_eq!(get(39).unwrap().activation, Activation::Held);
        assert_eq!(get(0).unwrap().activation.index(), 0);
        assert_eq!(get(12).unwrap().activation.index(), 2);
    }
}
