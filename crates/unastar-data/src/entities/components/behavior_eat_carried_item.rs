use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.eat_carried_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorEatCarriedItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
