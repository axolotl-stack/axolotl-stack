use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_around_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveAroundTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
