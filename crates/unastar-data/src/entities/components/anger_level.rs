use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:anger_level`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct AngerLevel {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
