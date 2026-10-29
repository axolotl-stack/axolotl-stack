//! English names BDS writes into command messages (`commands.kill.successful`
//! lists "Sheep, Chicken"; `commands.give.success` names the item).
//!
//! Names come from the vanilla `en_US.lang`: `entity.<id>.name` for actors,
//! `item.<id>.name` then `tile.<id>.name` for items. A few vanilla items use
//! description ids that differ from their identifier; those fall back to the
//! bare identifier until item description ids are part of the item data.

use crate::item::ItemStack;
use unastar_data::lang::en_us;

fn short(identifier: &str) -> &str {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier)
}

/// English name of an actor type (`minecraft:sheep` → `Sheep`).
pub fn entity_type_name(type_id: &str) -> String {
    let id = short(type_id);
    en_us()
        .get(&format!("entity.{id}.name"))
        .map_or_else(|| id.to_owned(), str::to_owned)
}

/// English name of an item identifier (`minecraft:wheat` → `Wheat`).
pub fn item_identifier_name(identifier: &str) -> String {
    let id = short(identifier);
    let lang = en_us();
    lang.get(&format!("item.{id}.name"))
        .or_else(|| lang.get(&format!("tile.{id}.name")))
        .map_or_else(|| id.to_owned(), str::to_owned)
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
        assert_eq!(
            item_identifier_name("minecraft:no_such_item"),
            "no_such_item"
        );
    }
}
