//! Potion effects by potion aux value, measured on BDS 1.26.51 by drinking
//! every potion (`data/measured/potion_effects.json`, see its `source`),
//! decoded once.
//!
//! Instant health and instant damage are recorded as BDS sends them, with
//! duration 1; callers apply them at once.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// One effect a potion gives when drunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PotionEffect {
    /// Protocol effect id.
    pub effect: u8,
    /// Zero-based amplifier.
    pub amplifier: u8,
    /// Duration in ticks (1 for instant effects).
    pub duration: u32,
}

/// One potion type.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PotionType {
    /// Potion aux value (the item's data).
    pub aux: i16,
    /// `minecraft:potion_type:<name>`.
    pub name: String,
    pub effects: Vec<PotionEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    source: String,
    potions: Vec<PotionType>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/measured/potion_effects.json"))
        .expect("embedded measured potion effects decode")
});

static BY_AUX: LazyLock<HashMap<i16, &'static PotionType>> =
    LazyLock::new(|| TABLE.potions.iter().map(|p| (p.aux, p)).collect());

/// Where the table was measured.
pub fn source() -> &'static str {
    &TABLE.source
}

/// Every measured potion type in aux order.
pub fn vanilla() -> &'static [PotionType] {
    &TABLE.potions
}

/// The potion type with aux `aux`.
pub fn get(aux: i16) -> Option<&'static PotionType> {
    BY_AUX.get(&aux).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_potion_aux_from_the_recipe_capture_is_measured() {
        let names: serde_json::Value =
            serde_json::from_str(include_str!("../data/measured/potion_types.json")).unwrap();
        for (name, aux) in names["types"].as_object().unwrap() {
            let aux = aux.as_i64().unwrap() as i16;
            let potion = get(aux).unwrap_or_else(|| panic!("{name} ({aux}) not measured"));
            assert_eq!(&potion.name, name);
        }
    }
}
