//! Melee damage of a player's held item, as measured on BDS 1.26.51.
//!
//! `data/measured/attack_damage.json` records, per tool key, the damage a
//! scripted player dealt to a fresh pig with that item in hand (see the
//! measured-data README). Tool keys are `hand` and the item name without the
//! `minecraft:` namespace; enchanted variants (`+sharpness1`) are kept in the
//! file as evidence but are not part of this table. Every item that is not
//! measured hits like an empty hand, as in vanilla.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize)]
struct Measurement {
    tool_item: Option<ToolItem>,
    damage: Vec<f32>,
}

#[derive(Deserialize)]
struct ToolItem {
    item: String,
    #[serde(default)]
    enchants: HashMap<String, u8>,
}

struct Table {
    hand: f32,
    by_item: HashMap<String, f32>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    let rows: HashMap<String, Measurement> =
        serde_json::from_str(include_str!("../data/measured/attack_damage.json"))
            .expect("measured attack damage decodes");
    let mut hand = None;
    let mut by_item = HashMap::new();
    for (key, row) in rows {
        let damage = *row
            .damage
            .first()
            .unwrap_or_else(|| panic!("attack damage {key} has no trial"));
        assert!(
            row.damage.iter().all(|trial| *trial == damage),
            "attack damage {key} trials disagree"
        );
        match row.tool_item {
            None => hand = Some(damage),
            Some(tool) if tool.enchants.is_empty() => {
                by_item.insert(tool.item, damage);
            }
            Some(_) => {}
        }
    }
    Table {
        hand: hand.expect("attack damage table measures the empty hand"),
        by_item,
    }
});

/// Damage a player's melee hit deals with `item` (`None`: empty hand).
pub fn attack_damage(item: Option<&str>) -> f32 {
    item.and_then(|item| TABLE.by_item.get(item))
        .copied()
        .unwrap_or(TABLE.hand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_weapons_and_the_empty_hand() {
        assert_eq!(attack_damage(None), 1.0);
        assert_eq!(attack_damage(Some("minecraft:diamond_sword")), 8.0);
        assert_eq!(attack_damage(Some("minecraft:wooden_axe")), 4.0);
        assert_eq!(attack_damage(Some("minecraft:dirt")), 1.0);
    }
}
