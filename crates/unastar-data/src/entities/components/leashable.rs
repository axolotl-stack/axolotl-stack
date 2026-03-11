use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:leashable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Leashable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
