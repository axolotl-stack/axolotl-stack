use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.timer_flag_1`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTimerFlag1 {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
