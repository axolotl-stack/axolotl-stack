use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:trail`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Trail {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
