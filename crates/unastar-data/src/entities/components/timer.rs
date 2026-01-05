use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:timer`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Timer {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
