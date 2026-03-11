use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.target_when_pushed`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTargetWhenPushed {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
