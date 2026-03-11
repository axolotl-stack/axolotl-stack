use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.jump_around_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorJumpAroundTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
