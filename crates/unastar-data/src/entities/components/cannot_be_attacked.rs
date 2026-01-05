use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:cannot_be_attacked`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct CannotBeAttacked {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
