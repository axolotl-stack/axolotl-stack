use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Attack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
