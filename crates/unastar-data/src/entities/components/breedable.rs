use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:breedable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Breedable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
