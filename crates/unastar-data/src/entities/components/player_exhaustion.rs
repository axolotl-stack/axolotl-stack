use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:player.exhaustion`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct PlayerExhaustion {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
