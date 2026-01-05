use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:player.level`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct PlayerLevel {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
