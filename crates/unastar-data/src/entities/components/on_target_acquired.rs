use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_target_acquired`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct OnTargetAcquired {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
