use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:nameable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Nameable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
