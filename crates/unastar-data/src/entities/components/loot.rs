use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:loot`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Loot {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
