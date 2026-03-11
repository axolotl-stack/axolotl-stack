use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.place_block`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlaceBlock {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
