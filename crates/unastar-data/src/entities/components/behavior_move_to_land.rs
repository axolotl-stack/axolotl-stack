use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_to_land`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveToLand {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
