use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:lava_movement`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct LavaMovement {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
