use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:buoyant`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Buoyant {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
