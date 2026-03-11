use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.swim_up_for_breath`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimUpForBreath {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
