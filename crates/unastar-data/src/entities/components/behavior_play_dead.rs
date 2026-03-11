use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.play_dead`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlayDead {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
