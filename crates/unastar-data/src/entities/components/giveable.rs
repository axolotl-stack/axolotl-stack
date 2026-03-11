use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:giveable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Giveable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
