use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.basic`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementBasic {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
