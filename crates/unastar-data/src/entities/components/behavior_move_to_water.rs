use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_to_water`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveToWater {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
