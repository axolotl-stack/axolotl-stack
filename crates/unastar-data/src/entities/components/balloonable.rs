use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:balloonable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Balloonable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
