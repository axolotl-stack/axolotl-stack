use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:rideable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Rideable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
