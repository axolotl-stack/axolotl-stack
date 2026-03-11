use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.jump_to_block`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorJumpToBlock {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
