//! The block BDS describes each item's stacks with, decoded once at startup.
//!
//! Source: `data/measured/item_blocks.json`, BDS 1.26.51 (the `item-blocks`
//! sweep of `tools/bds-sweep`: every item of the registry given with
//! `/replaceitem`, the block runtime id of the stack in the inventory BDS sent
//! next; method and hash in `data/measured/manifest.json`).
//!
//! A block item's stack names a block state, not always the block's first
//! palette state (a chest item describes `cardinal_direction=north`, an
//! amethyst bud `block_face=up`). Items that place a block without being
//! block items (doors, beds, signs, hoppers, cakes) describe no block.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde::Deserialize;

/// The block state a stack of an item describes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ItemBlock {
    /// Block identifier.
    pub block: String,
    /// State properties: strings, integers, or booleans for byte properties.
    pub states: BTreeMap<String, serde_json::Value>,
}

/// The measured table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ItemBlocks {
    /// Every item the sweep could give: its block, or `None` when it is no
    /// block item.
    pub items: BTreeMap<String, Option<ItemBlock>>,
    /// Items `/replaceitem` refuses (internal block items such as double
    /// slabs and wall signs); not measured.
    pub not_given: Vec<String>,
}

impl ItemBlocks {
    /// `Some(Some(block))` for a block item, `Some(None)` for an item that
    /// describes no block, `None` when the item was not measured.
    pub fn get(&self, item: &str) -> Option<Option<&ItemBlock>> {
        self.items.get(item).map(Option::as_ref)
    }
}

/// Decode the embedded table; consumers normally use [`item_blocks`].
pub fn decode_item_blocks() -> Result<ItemBlocks, serde_json::Error> {
    serde_json::from_str(include_str!("../data/measured/item_blocks.json"))
}

/// The measured item blocks of the vanilla items.
pub fn item_blocks() -> &'static ItemBlocks {
    static TABLE: LazyLock<ItemBlocks> =
        LazyLock::new(|| decode_item_blocks().expect("embedded BDS item-block table decodes"));
    &TABLE
}

/// The block state properties a creative entry's `block_states` names (base64
/// little-endian NBT), in the form of [`ItemBlock::states`]. The creative
/// inventory shows a few items with another state than a given stack of them
/// (pistons facing up, droppers and dispensers facing south, mushroom blocks
/// with caps on every side). `None` for a string that is not one of the
/// generated creative entries.
pub fn creative_block_states(
    encoded: &'static str,
) -> Option<&'static BTreeMap<String, serde_json::Value>> {
    static DECODED: LazyLock<HashMap<&'static str, BTreeMap<String, serde_json::Value>>> =
        LazyLock::new(|| {
            let data = crate::creative::CreativeInventoryData::load();
            data.all_groups_ordered()
                .into_iter()
                .flat_map(|(_, groups)| groups.iter())
                .flat_map(|group| group.items.iter().chain(group.group_icon.iter()))
                .filter_map(|entry| entry.block_states())
                .map(|encoded| {
                    let states = decode_block_states(encoded)
                        .unwrap_or_else(|| panic!("creative block states {encoded} decode"));
                    (encoded, states)
                })
                .collect()
        });
    DECODED.get(encoded)
}

fn decode_block_states(encoded: &str) -> Option<BTreeMap<String, serde_json::Value>> {
    use base64::Engine as _;
    use unastar_packs::nbt::{Encoding, Tag, read_compound};
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let Tag::Compound(states) =
        read_compound("creative block states", &bytes, Encoding::LittleEndian).ok()?
    else {
        return None;
    };
    states
        .into_iter()
        .map(|(name, tag)| {
            let value = match tag {
                Tag::Byte(bit) => serde_json::Value::Bool(bit != 0),
                Tag::Int(number) => serde_json::Value::from(number),
                Tag::String(text) => serde_json::Value::String(text),
                _ => return None,
            };
            Some((name, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_item_blocks_decode() {
        let table = item_blocks();
        let chest = table.get("minecraft:chest").unwrap().unwrap();
        assert_eq!(chest.block, "minecraft:chest");
        assert_eq!(
            chest.states["minecraft:cardinal_direction"],
            serde_json::json!("north")
        );
        assert_eq!(table.get("minecraft:wooden_door"), Some(None));
        assert!(table.get("minecraft:oak_double_slab").is_none());
        assert!(
            table
                .not_given
                .contains(&"minecraft:oak_double_slab".to_owned())
        );
    }

    #[test]
    fn creative_block_states_decode() {
        let piston = crate::creative::CreativeInventoryData::load()
            .all_groups_ordered()
            .into_iter()
            .flat_map(|(_, groups)| groups.iter())
            .flat_map(|group| group.items.iter())
            .find(|entry| entry.name == "minecraft:piston")
            .and_then(|entry| entry.block_states())
            .unwrap();
        let states = creative_block_states(piston).unwrap();
        assert_eq!(states["facing_direction"], serde_json::json!(1));
    }
}
