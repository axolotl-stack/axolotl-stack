use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.roar`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRoar {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
