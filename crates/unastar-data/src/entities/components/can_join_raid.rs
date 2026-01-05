use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:can_join_raid`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct CanJoinRaid {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
