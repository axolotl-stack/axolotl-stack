//! English names BDS writes into command messages (`commands.kill.successful`
//! lists "Sheep, Chicken"; `commands.give.success` names the item).
//!
//! Names come from the vanilla `en_US.lang`: `entity.<id>.name` for actors,
//! `item.<id>.name` then `tile.<id>.name` for items. Flattened block items
//! keep their pre-flattening language key (`minecraft:red_wool` is
//! `tile.wool.red.name`, `minecraft:stone` is `tile.stone.stone.name`), which no
//! data source in the repository maps yet; for those the identifier is title
//! cased ("Red Wool", "Stone"), which is the English name in the cases measured
//! on BDS (`/give @s wool 3 14` → "Red Wool").

use crate::item::ItemStack;
use unastar_data::lang::en_us;

fn short(identifier: &str) -> &str {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier)
}

/// `red_wool` → `Red Wool`.
fn title_case(id: &str) -> String {
    id.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// English name of an actor type (`minecraft:sheep` → `Sheep`).
pub fn entity_type_name(type_id: &str) -> String {
    let id = short(type_id);
    en_us()
        .get(&format!("entity.{id}.name"))
        .map_or_else(|| title_case(id), str::to_owned)
}

/// English name of an item identifier (`minecraft:wheat` → `Wheat`).
pub fn item_identifier_name(identifier: &str) -> String {
    let id = short(identifier);
    let lang = en_us();
    lang.get(&format!("item.{id}.name"))
        .or_else(|| lang.get(&format!("tile.{id}.name")))
        .map_or_else(|| title_case(id), str::to_owned)
}

/// English name of a stack's item.
pub fn item_name(stack: &ItemStack) -> String {
    item_identifier_name(stack.item_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_through_the_vanilla_lang() {
        assert_eq!(entity_type_name("minecraft:sheep"), "Sheep");
        assert_eq!(item_identifier_name("minecraft:wheat"), "Wheat");
        assert_eq!(item_identifier_name("minecraft:wheat_seeds"), "Wheat Seeds");
        assert_eq!(item_identifier_name("minecraft:azalea"), "Azalea");
        // Flattened block items without a lang key of their own (measured
        // on BDS: "Red Wool", "Stone").
        assert_eq!(item_identifier_name("minecraft:red_wool"), "Red Wool");
        assert_eq!(item_identifier_name("minecraft:stone"), "Stone");
    }
}
