use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:insomnia`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Insomnia {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
