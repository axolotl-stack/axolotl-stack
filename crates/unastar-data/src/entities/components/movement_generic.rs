use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.generic`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementGeneric {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
