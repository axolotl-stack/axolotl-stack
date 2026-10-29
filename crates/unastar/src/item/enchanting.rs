//! How vanilla items take random enchantments: the enchantment slots and
//! enchantability each BDS item class reports.
//!
//! Tools use their tier's enchantability (wood 15, stone 5, copper 13,
//! iron 14, diamond 10, gold 22, netherite 15), humanoid armour its
//! material's (leather 15, chainmail 12, copper 8, iron 9, gold 25,
//! diamond 10, netherite 15, turtle 9, elytra 0); bows, crossbows and
//! tridents have 1. Data-driven items carry `minecraft:enchantable`.

use std::collections::HashMap;
use std::sync::LazyLock;

use unastar_data::armour::{ArmourSlot, armour_slot};
use unastar_rules::enchanting::{ItemEnchanting, slot};

use crate::registry::item::{ToolKind, ToolTier, tool_info};

/// Armour material enchantability, by the item's tier tag.
const ARMOUR_MATERIALS: [(&str, i32); 7] = [
    ("minecraft:leather_tier", 15),
    ("minecraft:chainmail_tier", 12),
    ("minecraft:copper_tier", 8),
    ("minecraft:iron_tier", 9),
    ("minecraft:golden_tier", 25),
    ("minecraft:diamond_tier", 10),
    ("minecraft:netherite_tier", 15),
];
const TURTLE_HELMET: &str = "minecraft:turtle_helmet";
const TURTLE_ENCHANTABILITY: i32 = 9;
const ELYTRA: &str = "minecraft:elytra";
/// Ranged weapons and tridents.
const RANGED: [(&str, u32); 3] = [
    ("minecraft:bow", slot::BOW),
    ("minecraft:crossbow", slot::CROSSBOW),
    ("minecraft:trident", slot::TRIDENT),
];
const RANGED_ENCHANTABILITY: i32 = 1;

/// `minecraft:enchantable` of the vanilla data-driven item documents.
static DATA_DRIVEN: LazyLock<HashMap<String, (String, i32)>> = LazyLock::new(|| {
    unastar_data::item_components::decode_vanilla()
        .expect("vanilla item documents decode")
        .into_iter()
        .filter_map(|item| {
            let enchantable = item.components.enchantable?;
            Some((item.identifier, (enchantable.slot, enchantable.value)))
        })
        .collect()
});

fn tier_enchantability(tier: ToolTier) -> i32 {
    match tier {
        ToolTier::Wooden => 15,
        ToolTier::Golden => 22,
        ToolTier::Stone => 5,
        ToolTier::Copper => 13,
        ToolTier::Iron => 14,
        ToolTier::Diamond => 10,
        ToolTier::Netherite => 15,
    }
}

/// Slot bits of a `minecraft:enchantable` slot name used by vanilla items.
fn component_slot(name: &str) -> Option<u32> {
    Some(match name {
        "none" => 0,
        "melee_spear" => slot::MELEE_SPEAR,
        _ => return None,
    })
}

/// Enchantment slots and enchantability of `item_id`; `None` when the item
/// is not of a classified vanilla item class.
pub fn item_enchanting(item_id: &str) -> Option<ItemEnchanting> {
    if let Some((slot_name, value)) = DATA_DRIVEN.get(item_id) {
        return Some(ItemEnchanting {
            slots: component_slot(slot_name)?,
            value: *value,
        });
    }
    if let Some((_, slots)) = RANGED.iter().find(|(id, _)| *id == item_id) {
        return Some(ItemEnchanting {
            slots: *slots,
            value: RANGED_ENCHANTABILITY,
        });
    }
    if item_id == ELYTRA {
        return Some(ItemEnchanting {
            slots: slot::ELYTRA,
            value: 0,
        });
    }
    if let Some(worn) = armour_slot(item_id) {
        let slots = match worn {
            ArmourSlot::Head => slot::ARMOR_HEAD,
            ArmourSlot::Chest => slot::ARMOR_TORSO,
            ArmourSlot::Legs => slot::ARMOR_LEGS,
            ArmourSlot::Feet => slot::ARMOR_FEET,
        };
        let value = if item_id == TURTLE_HELMET {
            TURTLE_ENCHANTABILITY
        } else {
            let data = crate::registry::item::item_data(item_id)?;
            ARMOUR_MATERIALS
                .iter()
                .find(|(tag, _)| data.has_tag(tag))
                .map(|(_, value)| *value)?
        };
        return Some(ItemEnchanting { slots, value });
    }
    let tool = tool_info(item_id)?;
    let slots = match tool.kind? {
        ToolKind::Sword => slot::SWORD,
        ToolKind::Shovel => slot::SHOVEL,
        ToolKind::Pickaxe => slot::PICKAXE,
        ToolKind::Axe => slot::AXE,
        ToolKind::Hoe => slot::HOE,
        ToolKind::Shears => return None,
    };
    Some(ItemEnchanting {
        slots,
        value: tier_enchantability(tool.tier?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(item: &str) -> (u32, i32) {
        let facts = item_enchanting(item).unwrap_or_else(|| panic!("{item} unclassified"));
        (facts.slots, facts.value)
    }

    #[test]
    fn zombie_gear_is_classified_like_bds() {
        assert_eq!(facts("minecraft:iron_sword"), (slot::SWORD, 14));
        assert_eq!(facts("minecraft:iron_shovel"), (slot::SHOVEL, 14));
        assert_eq!(facts("minecraft:iron_spear"), (slot::MELEE_SPEAR, 14));
        assert_eq!(facts("minecraft:golden_spear"), (slot::MELEE_SPEAR, 22));
        assert_eq!(facts("minecraft:leather_helmet"), (slot::ARMOR_HEAD, 15));
        assert_eq!(facts("minecraft:copper_chestplate"), (slot::ARMOR_TORSO, 8));
        assert_eq!(facts("minecraft:golden_leggings"), (slot::ARMOR_LEGS, 25));
        assert_eq!(facts("minecraft:chainmail_boots"), (slot::ARMOR_FEET, 12));
        assert_eq!(facts("minecraft:iron_helmet"), (slot::ARMOR_HEAD, 9));
        assert_eq!(facts("minecraft:diamond_boots"), (slot::ARMOR_FEET, 10));
        assert_eq!(facts("minecraft:turtle_helmet"), (slot::ARMOR_HEAD, 9));
        assert_eq!(facts("minecraft:bow"), (slot::BOW, 1));
        assert_eq!(facts("minecraft:golden_sword"), (slot::SWORD, 22));
        assert_eq!(item_enchanting("minecraft:rotten_flesh"), None);
    }
}
