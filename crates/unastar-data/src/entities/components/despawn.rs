use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:despawn`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Despawn {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
