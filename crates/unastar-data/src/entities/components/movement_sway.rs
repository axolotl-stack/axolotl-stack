use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.sway`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementSway {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
