use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_towards_home_restriction`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveTowardsHomeRestriction {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
