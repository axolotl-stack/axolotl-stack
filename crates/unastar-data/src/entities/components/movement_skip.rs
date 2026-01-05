use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.skip`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementSkip {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
