use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.slime_keep_on_jumping`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSlimeKeepOnJumping {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
