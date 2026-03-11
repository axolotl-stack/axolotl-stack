use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.sniff`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSniff {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
