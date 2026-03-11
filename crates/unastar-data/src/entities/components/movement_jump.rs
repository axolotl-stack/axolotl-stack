use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.jump`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementJump {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
