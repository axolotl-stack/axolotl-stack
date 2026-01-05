use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.leap_at_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorLeapAtTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
