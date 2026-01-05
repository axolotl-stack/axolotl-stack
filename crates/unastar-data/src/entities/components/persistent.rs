use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:persistent`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Persistent {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
