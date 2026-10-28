//! Measured furnace fuel burn times, decoded once at startup.
//!
//! Source: `data/measured/fuel_burn_ticks.json`, BDS 1.26.51: a furnace with
//! one cobblestone input and the item as fuel, server ticks from lit to unlit
//! (method and hashes in `data/measured/manifest.json`). Every registry item
//! was swept; an item absent from the table does not light a furnace.
//!
//! The values are furnace ticks (cook interval 200). BDS scales every fuel by
//! the block's cook interval, so a blast furnace or smoker (100) burns each
//! fuel for half as many ticks; see [`burn_ticks`].

use std::collections::HashMap;
use std::sync::LazyLock;

/// The furnace cook interval the table was measured with.
pub const MEASURED_COOK_TICKS: u32 = 200;

/// Decode the embedded table; consumers normally use [`furnace_ticks`].
pub fn decode_vanilla() -> Result<HashMap<String, u32>, serde_json::Error> {
    serde_json::from_str(include_str!("../data/measured/fuel_burn_ticks.json"))
}

fn vanilla() -> &'static HashMap<String, u32> {
    static TICKS: LazyLock<HashMap<String, u32>> =
        LazyLock::new(|| decode_vanilla().expect("embedded BDS fuel table decodes"));
    &TICKS
}

/// Measured furnace burn ticks of a vanilla item, `None` if it is not fuel.
pub fn furnace_ticks(identifier: &str) -> Option<u32> {
    vanilla().get(identifier).copied()
}

/// Burn ticks of a vanilla item in a block with `cook_ticks` per smelt.
pub fn burn_ticks(identifier: &str, cook_ticks: u32) -> Option<u32> {
    furnace_ticks(identifier).map(|ticks| ticks * cook_ticks / MEASURED_COOK_TICKS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_fuels_scale_with_the_cook_interval() {
        assert_eq!(furnace_ticks("minecraft:coal"), Some(1600));
        assert_eq!(burn_ticks("minecraft:coal", 100), Some(800));
        assert_eq!(furnace_ticks("minecraft:lava_bucket"), Some(20000));
        assert_eq!(furnace_ticks("minecraft:cobblestone"), None);
        assert_eq!(furnace_ticks("minecraft:bucket"), None);
    }
}
