use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:rail_movement`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct RailMovement {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
