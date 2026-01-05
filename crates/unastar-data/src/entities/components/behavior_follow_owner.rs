use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.follow_owner`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorFollowOwner {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
