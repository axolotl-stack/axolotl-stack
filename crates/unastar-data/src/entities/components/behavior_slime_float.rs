use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.slime_float`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSlimeFloat {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
