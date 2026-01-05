use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_towards_dwelling_restriction`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveTowardsDwellingRestriction {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
