use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:home`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Home {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
