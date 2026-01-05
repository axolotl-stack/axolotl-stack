use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:body_rotation_always_follows_head`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BodyRotationAlwaysFollowsHead {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
