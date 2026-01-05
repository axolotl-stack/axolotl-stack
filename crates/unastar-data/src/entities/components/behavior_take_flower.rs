use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.take_flower`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorTakeFlower {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
