use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.avoid_block`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorAvoidBlock {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
