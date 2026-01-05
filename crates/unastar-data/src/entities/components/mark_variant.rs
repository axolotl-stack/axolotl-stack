use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:mark_variant`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MarkVariant {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
