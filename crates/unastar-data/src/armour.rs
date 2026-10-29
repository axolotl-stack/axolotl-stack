//! Armour defence and toughness per wearable item, and the damage they let
//! through, as measured on BDS 1.26.51.
//!
//! `data/measured/armor_values.json` records, for every `minecraft:is_armor`
//! item worn alone, the health a player lost per `entity_attack` hit of 4, 16
//! and 1000 points (see the measured-data README). A 1000-point hit is
//! clamped to `defence / 5` points of reduction, which gives the defence
//! exactly; the small hits bound the toughness.
//!
//! Toughness is not read out exactly by the sweep. The values here are
//! provisional (Minecraft Wiki "Armor": diamond 2 per piece, netherite 3,
//! every other material 0); a test checks each one lies inside the interval
//! the sweep measured and that [`damage_after_armour`] reproduces every
//! measured hit.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// The body slot a piece of armour is worn in (the measured `slot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmourSlot {
    Head,
    Chest,
    Legs,
    Feet,
}

/// Defence points, toughness and slot of one worn item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmourValues {
    pub defence: f32,
    pub toughness: f32,
    pub slot: ArmourSlot,
}

#[derive(Deserialize)]
struct Measurement {
    defense: f32,
    slot: ArmourSlot,
}

/// Provisional toughness per item-name prefix (Minecraft Wiki "Armor").
const TOUGHNESS: [(&str, f32); 2] = [("minecraft:diamond_", 2.0), ("minecraft:netherite_", 3.0)];

static TABLE: LazyLock<HashMap<String, ArmourValues>> = LazyLock::new(|| {
    let rows: HashMap<String, Measurement> =
        serde_json::from_str(include_str!("../data/measured/armor_values.json"))
            .expect("measured armour values decode");
    rows.into_iter()
        .map(|(item, row)| {
            let toughness = TOUGHNESS
                .iter()
                .find(|(prefix, _)| item.starts_with(prefix))
                .map_or(0.0, |(_, toughness)| *toughness);
            (
                item,
                ArmourValues {
                    defence: row.defense,
                    toughness,
                    slot: row.slot,
                },
            )
        })
        .collect()
});

/// Defence and toughness of `item` when worn; `None` for items that give
/// no armour.
pub fn armour_values(item: &str) -> Option<ArmourValues> {
    TABLE.get(item).copied()
}

/// The slot `item` is worn in; `None` for items that give no armour.
pub fn armour_slot(item: &str) -> Option<ArmourSlot> {
    TABLE.get(item).map(|values| values.slot)
}

/// Damage left of `damage` after armour with the summed `defence` and
/// `toughness` of every worn piece:
/// `damage × (1 − clamp(max(defence / 5, defence − damage / (2 + toughness / 4)), 0, 20) / 25)`.
pub fn damage_after_armour(damage: f32, defence: f32, toughness: f32) -> f32 {
    let reduction = (defence - damage / (2.0 + toughness / 4.0))
        .max(defence / 5.0)
        .clamp(0.0, 20.0);
    damage * (1.0 - reduction / 25.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Row {
        defense: f32,
        per_hit: HashMap<String, f32>,
        toughness_range: Option<HashMap<String, (f32, f32)>>,
    }

    fn rows() -> HashMap<String, Row> {
        serde_json::from_str(include_str!("../data/measured/armor_values.json")).unwrap()
    }

    #[test]
    fn measured_defence_per_item() {
        assert_eq!(
            armour_values("minecraft:diamond_chestplate"),
            Some(ArmourValues {
                defence: 8.0,
                toughness: 2.0,
                slot: ArmourSlot::Chest,
            })
        );
        assert_eq!(
            armour_values("minecraft:netherite_leggings"),
            Some(ArmourValues {
                defence: 6.0,
                toughness: 3.0,
                slot: ArmourSlot::Legs,
            })
        );
        assert_eq!(
            armour_values("minecraft:leather_boots").map(|a| a.defence),
            Some(1.0)
        );
        assert_eq!(armour_values("minecraft:dirt"), None);
    }

    /// The sweep reads health in whole points summed over 10 or 20 hits,
    /// so each measured per-hit loss is the formula's value rounded down
    /// to a tenth or a twentieth of a point.
    #[test]
    fn formula_reproduces_every_measured_hit() {
        for (item, row) in rows() {
            let values = armour_values(&item).unwrap();
            assert_eq!(values.defence, row.defense, "{item}");
            for (damage, measured) in &row.per_hit {
                let damage: f32 = damage.parse().unwrap();
                let expected = damage_after_armour(damage, values.defence, values.toughness);
                let gap = expected - measured;
                assert!(
                    (-1.0e-3..0.1).contains(&gap),
                    "{item} hit {damage}: formula {expected}, measured {measured}"
                );
            }
        }
    }

    #[test]
    fn toughness_lies_in_every_measured_interval() {
        for (item, row) in rows() {
            let toughness = armour_values(&item).unwrap().toughness;
            for (damage, (low, high)) in row.toughness_range.into_iter().flatten() {
                assert!(
                    (low..=high).contains(&toughness),
                    "{item} hit {damage}: toughness {toughness} outside [{low}, {high}]"
                );
            }
        }
    }
}
