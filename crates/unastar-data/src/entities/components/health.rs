use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:health`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Health {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
