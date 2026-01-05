use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.fire_at_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorFireAtTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
