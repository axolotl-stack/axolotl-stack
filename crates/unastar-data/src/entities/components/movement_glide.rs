use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.glide`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementGlide {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
