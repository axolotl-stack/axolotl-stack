use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.look_at_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorLookAtTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
