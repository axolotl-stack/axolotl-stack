use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_start_landing`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct OnStartLanding {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
