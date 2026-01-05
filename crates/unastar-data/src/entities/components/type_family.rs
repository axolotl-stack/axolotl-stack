use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:type_family`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct TypeFamily {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
