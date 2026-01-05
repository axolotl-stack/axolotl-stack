use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:admire_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct AdmireItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
