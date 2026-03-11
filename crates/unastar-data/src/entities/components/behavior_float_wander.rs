use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.float_wander`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFloatWander {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
