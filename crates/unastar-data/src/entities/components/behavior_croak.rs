use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.croak`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorCroak {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
