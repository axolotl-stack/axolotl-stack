use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:teleport`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Teleport {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
