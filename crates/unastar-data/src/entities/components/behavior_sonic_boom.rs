use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.sonic_boom`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSonicBoom {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
