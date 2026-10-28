//! Measured built-in armour slots and protection, decoded once at startup.
//!
//! Source: `data/measured/armor_values.json`, BDS 1.26.51, with the sweep
//! method and hashes in `data/measured/manifest.json` (`armor_values`). The
//! slot is the slot used while measuring one equipped item, including elytra
//! with zero defense. `armor_slots.json` is deliberately not used: the command
//! path accepts arbitrary head/offhand items and is not inventory permission
//! evidence. Native head/offhand capabilities live in [`crate::items::ItemData`].

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// The four player armour slots, shared by measurements and compiled rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmourSlot {
    Head,
    Chest,
    Legs,
    Feet,
}

impl ArmourSlot {
    /// Player inventory armour index (not a protocol container slot).
    pub const fn player_slot(self) -> usize {
        match self {
            Self::Head => 0,
            Self::Chest => 1,
            Self::Legs => 2,
            Self::Feet => 3,
        }
    }

    /// Reject indices outside the four player armour slots.
    pub const fn from_player_slot(slot: usize) -> Option<Self> {
        match slot {
            0 => Some(Self::Head),
            1 => Some(Self::Chest),
            2 => Some(Self::Legs),
            3 => Some(Self::Feet),
            _ => None,
        }
    }
}

/// One measurement row. Toughness is an observed interval, not an exact stat.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmourValues {
    pub slot: ArmourSlot,
    pub defense: f64,
    /// Damage input -> observed health lost, with only this item equipped.
    pub per_hit: BTreeMap<u32, f64>,
    /// Damage input -> inferred toughness interval; null means unresolved.
    pub toughness_range: Option<BTreeMap<u32, [f64; 2]>>,
}

/// Decode the embedded family; consumers normally use [`get`] at registry load.
pub fn decode_vanilla() -> Result<HashMap<String, ArmourValues>, serde_json::Error> {
    serde_json::from_str(include_str!("../data/measured/armor_values.json"))
}

/// All measured rows, decoded and indexed once on first startup access.
pub fn vanilla() -> &'static HashMap<String, ArmourValues> {
    static VALUES: LazyLock<HashMap<String, ArmourValues>> =
        LazyLock::new(|| decode_vanilla().expect("embedded BDS armour values decode"));
    &VALUES
}

/// Identifier lookup for registry compilation only, never inventory placement.
pub fn get(identifier: &str) -> Option<&'static ArmourValues> {
    vanilla().get(identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_slot_and_defense_are_not_derived_from_item_names() {
        let chest = get("minecraft:iron_chestplate").unwrap();
        assert_eq!(chest.slot, ArmourSlot::Chest);
        assert_eq!(chest.defense, 6.0);
        assert_eq!(chest.per_hit[&1000], 952.0);
        let elytra = get("minecraft:elytra").unwrap();
        assert_eq!(elytra.slot, ArmourSlot::Chest);
        assert_eq!(elytra.defense, 0.0);
        assert_eq!(elytra.toughness_range, None);
        let netherite = get("minecraft:netherite_chestplate").unwrap();
        assert_eq!(
            netherite.toughness_range.as_ref().unwrap()[&16],
            [2.72, 3.31]
        );
    }

    #[test]
    fn typed_rows_accept_neutral_identifiers_and_reject_unknown_slots() {
        let rows: HashMap<String, ArmourValues> = serde_json::from_str(
            r#"{"addon:plain":{"slot":"feet","defense":2.0,"per_hit":{"4":3.9},"toughness_range":null}}"#,
        )
        .unwrap();
        assert_eq!(rows["addon:plain"].slot, ArmourSlot::Feet);
        assert_eq!(rows["addon:plain"].defense, 2.0);
        assert!(
            serde_json::from_str::<ArmourValues>(
                r#"{"slot":"unknown","defense":0,"per_hit":{},"toughness_range":null}"#,
            )
            .is_err()
        );
    }

    #[test]
    fn player_slot_boundaries_do_not_wrap() {
        assert_eq!(ArmourSlot::from_player_slot(0), Some(ArmourSlot::Head));
        assert_eq!(ArmourSlot::from_player_slot(3), Some(ArmourSlot::Feet));
        assert_eq!(ArmourSlot::from_player_slot(4), None);
        assert_eq!(ArmourSlot::from_player_slot(usize::MAX), None);
        for slot in [
            ArmourSlot::Head,
            ArmourSlot::Chest,
            ArmourSlot::Legs,
            ArmourSlot::Feet,
        ] {
            assert_eq!(ArmourSlot::from_player_slot(slot.player_slot()), Some(slot));
        }
    }
}
