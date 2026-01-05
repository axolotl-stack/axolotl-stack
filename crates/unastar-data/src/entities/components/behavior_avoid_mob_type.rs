use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.avoid_mob_type`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorAvoidMobType {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
