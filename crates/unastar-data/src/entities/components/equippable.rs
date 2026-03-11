use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:equippable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Equippable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
