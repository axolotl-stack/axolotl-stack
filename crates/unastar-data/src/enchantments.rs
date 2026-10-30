//! Enchantment table for the anvil, grindstone and enchanting table
//! (`data/measured/enchantments.json`, public reference, see its `source`),
//! decoded once.
//!
//! Each enchantment lists the item categories that take it; a category is an
//! armour slot, item tags, item identifiers or "any item with durability".
//! The enchanting table adds a selection weight, the cost range of each
//! level and the item enchantability.

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
    /// Selection weight among the candidates of a random enchant.
    pub weight: u32,
    /// Inclusive enchantment-cost range of each level (index 0 is level 1)
    /// in which random enchanting picks that level; the highest matching
    /// level wins. Empty: random enchanting never selects it.
    pub costs: Vec<[i32; 2]>,
    /// Never offered by the enchanting table; random loot may still pick it
    /// when it allows treasure.
    pub treasure: bool,
    /// The enchanting-table option list that carries it.
    pub activation: Activation,
}

impl Enchantment {
    /// Minimum enchanting level of `level`.
    pub fn min_cost(&self, level: i16) -> i32 {
        self.min_cost_base + (i32::from(level) - 1) * self.min_cost_per_level
    }

    /// The highest level whose cost range contains `cost`, if any.
    pub fn level_for_cost(&self, cost: i32) -> Option<i16> {
        self.costs
            .iter()
            .rposition(|[min, max]| (*min..=*max).contains(&cost))
            .map(|index| index as i16 + 1)
    }
}

/// Items sharing one enchantability.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnchantabilityRow {
    value: i32,
    items: Vec<String>,
}

/// The enchant-slot bit of the items in one category.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotMask {
    /// Category name (a key of [`category`]).
    pub category: String,
    pub mask: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    categories: HashMap<String, Category>,
    enchantments: Vec<Enchantment>,
    incompatible: Vec<Vec<i16>>,
    enchantability: Vec<EnchantabilityRow>,
    slot_masks: Vec<SlotMask>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/enchantments.json"))
        .expect("embedded enchantment table decodes")
});

static BY_ID: LazyLock<HashMap<i16, &'static Enchantment>> =
    LazyLock::new(|| TABLE.enchantments.iter().map(|e| (e.id, e)).collect());

static ENCHANTABILITY: LazyLock<HashMap<&'static str, i32>> = LazyLock::new(|| {
    TABLE
        .enchantability
        .iter()
        .flat_map(|row| row.items.iter().map(|item| (item.as_str(), row.value)))
        .collect()
});

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

/// Enchanting-table enchantability of item `identifier`; 0 for items the
/// table cannot enchant.
pub fn enchantability(identifier: &str) -> i32 {
    ENCHANTABILITY.get(identifier).copied().unwrap_or(0)
}

/// Enchant-slot bits in priority order: an item takes the first category
/// it is in.
pub fn slot_masks() -> &'static [SlotMask] {
    &TABLE.slot_masks
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
            // Quick Charge III has no reachable range (minecraft.wiki).
            assert!(
                enchantment.treasure || !enchantment.costs.is_empty(),
                "{}",
                enchantment.name
            );
            assert!(enchantment.weight > 0);
        }
        for group in &TABLE.incompatible {
            for id in group {
                assert!(get(*id).is_some(), "{id}");
            }
        }
        for slot in slot_masks() {
            assert!(category(&slot.category).is_some(), "{}", slot.category);
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
    fn cost_ranges_pick_the_highest_level() {
        let sharpness = get(9).unwrap();
        assert_eq!(sharpness.level_for_cost(0), None);
        assert_eq!(sharpness.level_for_cost(1), Some(1));
        assert_eq!(sharpness.level_for_cost(23), Some(3));
        assert_eq!(sharpness.level_for_cost(65), Some(5));
        assert_eq!(sharpness.level_for_cost(66), None);
        assert_eq!(get(26).unwrap().level_for_cost(30), Some(1));
        assert_eq!(get(36).unwrap().level_for_cost(30), None);
    }

    #[test]
    fn enchantability_follows_material_and_kind() {
        assert_eq!(enchantability("minecraft:golden_helmet"), 25);
        assert_eq!(enchantability("minecraft:golden_sword"), 22);
        assert_eq!(enchantability("minecraft:iron_sword"), 14);
        assert_eq!(enchantability("minecraft:iron_chestplate"), 9);
        assert_eq!(enchantability("minecraft:mace"), 15);
        assert_eq!(enchantability("minecraft:netherite_spear"), 15);
        assert_eq!(enchantability("minecraft:book"), 1);
        assert_eq!(enchantability("minecraft:shears"), 0);
        assert_eq!(enchantability("minecraft:netherite_horse_armor"), 0);
        assert_eq!(get(39).unwrap().activation, Activation::Held);
        assert_eq!(get(0).unwrap().activation.index(), 0);
        assert_eq!(get(12).unwrap().activation.index(), 2);
    }
}
