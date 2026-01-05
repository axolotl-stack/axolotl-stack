use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:equipment`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Equipment {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
