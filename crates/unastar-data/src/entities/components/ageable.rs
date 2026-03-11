use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:ageable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Ageable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
