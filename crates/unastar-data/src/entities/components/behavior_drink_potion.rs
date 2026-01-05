use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.drink_potion`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorDrinkPotion {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
