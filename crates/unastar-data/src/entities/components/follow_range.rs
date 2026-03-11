use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:follow_range`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct FollowRange {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
