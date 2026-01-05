use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.hover`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementHover {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
