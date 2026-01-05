use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:variant`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Variant {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
