use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct GiveableTriggersOnGive {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for GiveableTriggersOnGive {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct GiveableTriggers {
    ///An optional cool down in seconds to prevent spamming interactions.
    pub cooldown: Option<f32>,
    ///The list of items that can be given to the entity to place in their inventory.
    pub items: Option<Vec<crate::types::BedrockValue>>,
    ///Event to fire when the correct item is given.
    pub on_give: Option<GiveableTriggersOnGive>,
}
impl Default for GiveableTriggers {
    fn default() -> Self {
        Self {
            cooldown: None,
            items: None,
            on_give: None,
        }
    }
}
/// Bedrock component `minecraft:giveable`. Defines sets of items that can be used to trigger events when used on this entity. The item will also be taken and placed in the entity's inventory.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Giveable {
    ///Defines sets of items that can be used to trigger events when used on this entity. The item will also be taken and placed in the entity's inventory.
    pub triggers: Option<GiveableTriggers>,
}
impl Default for Giveable {
    fn default() -> Self {
        Self { triggers: None }
    }
}
