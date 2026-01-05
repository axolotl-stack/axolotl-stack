use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:is_stackable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct IsStackable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
