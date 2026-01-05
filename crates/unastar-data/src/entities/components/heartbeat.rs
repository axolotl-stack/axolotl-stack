use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:heartbeat`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Heartbeat {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
