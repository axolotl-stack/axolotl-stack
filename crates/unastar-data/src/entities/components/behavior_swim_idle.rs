use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.swim_idle`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimIdle {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
