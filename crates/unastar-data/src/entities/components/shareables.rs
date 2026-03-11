use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct ShareablesItems {
    ///Mob will admire the item after picking up by looking at it. For this to happen the mob needs to have an Admire component and an Admire goal.
    pub admire: Option<bool>,
    ///Mob will barter for the item after picking it up. For this to work the mob needs to have a Barter component and a Barter goal.
    pub barter: Option<bool>,
    ///Determines whether the mob will consume the item or not.
    pub consume_item: Option<bool>,
    ///Defines the item this entity wants to craft with the item defined above. Should be an item name.
    pub craft_into: Option<String>,
    ///The name of the item.
    pub item: Option<String>,
    ///Aux value for the item.
    pub item_aux: Option<i32>,
    ///Maximum number of this item the mob will hold.
    pub max_amount: Option<i32>,
    ///Maximum number of this item the mob will pick up during a single goal tick.
    pub pickup_limit: Option<i32>,
    ///Determines whether the mob can only pickup the item and not drop it.
    pub pickup_only: Option<bool>,
    ///Prioritizes which items the entity prefers. 0 is the highest priority.
    pub priority: Option<i32>,
    ///Determines whether the mob will try to put the item in its inventory if it has the inventory component and if it can't be equipped.
    pub stored_in_inventory: Option<bool>,
    ///Number of this item considered extra that the entity wants to share.
    pub surplus_amount: Option<i32>,
    ///Number of this item this entity wants to have.
    pub want_amount: Option<i32>,
}
impl Default for ShareablesItems {
    fn default() -> Self {
        Self {
            admire: None,
            barter: None,
            consume_item: None,
            craft_into: None,
            item: None,
            item_aux: None,
            max_amount: None,
            pickup_limit: None,
            pickup_only: None,
            priority: None,
            stored_in_inventory: None,
            surplus_amount: None,
            want_amount: None,
        }
    }
}
/// Bedrock component `minecraft:shareables`. Defines a list of items the mob wants to share or pick up. Each item must have the following parameters:
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Shareables {
    ///A bucket for all other items in the game. Note this category is always least priority items.
    pub all_items: Option<bool>,
    ///Maximum number of this item the mob will hold.
    pub all_items_max_amount: Option<i32>,
    ///Number of this item considered extra that the entity wants to share.
    pub all_items_surplus_amount: Option<i32>,
    ///Number of this item this entity wants to share.
    pub all_items_want_amount: Option<i32>,
    ///List of items that the entity wants to share.
    pub items: Option<Vec<ShareablesItems>>,
    ///Determines whether the mob can only pickup one item at a time.
    pub singular_pickup: Option<bool>,
}
impl Default for Shareables {
    fn default() -> Self {
        Self {
            all_items: Some(false),
            all_items_max_amount: Some(-1i32),
            all_items_surplus_amount: Some(-1i32),
            all_items_want_amount: Some(-1i32),
            items: None,
            singular_pickup: Some(false),
        }
    }
}
