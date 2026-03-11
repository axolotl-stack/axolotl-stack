use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct AgeableGrowUp {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for AgeableGrowUp {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:ageable`. Adds a timer for the entity to grow up. It can be accelerated by giving the entity the items it likes as defined by feedItems.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Ageable {
    ///List of items that the entity drops when it grows up.
    pub drop_items: Option<Vec<crate::types::BedrockValue>>,
    ///Amount of time before the entity grows up, -1 for always a baby.
    pub duration: Option<f32>,
    ///List of items that can be fed to the entity. Includes `item` for the item name and `growth` to define how much time it grows up by
    pub feed_items: Option<crate::types::BedrockValue>,
    ///Event to run when this entity grows up.
    pub grow_up: Option<AgeableGrowUp>,
    ///List of conditions to meet so that the entity can be fed.
    pub interact_filters: Option<crate::types::BedrockValue>,
}
impl Default for Ageable {
    fn default() -> Self {
        Self {
            drop_items: None,
            duration: Some(1200f32),
            feed_items: None,
            grow_up: None,
            interact_filters: None,
        }
    }
}
