use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.look_at_player`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorLookAtPlayer {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
