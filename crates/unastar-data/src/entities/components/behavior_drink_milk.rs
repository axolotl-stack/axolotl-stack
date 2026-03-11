use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.drink_milk`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorDrinkMilk {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
