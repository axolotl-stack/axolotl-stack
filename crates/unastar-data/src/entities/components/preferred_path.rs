use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:preferred_path`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PreferredPath {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
