use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_start_takeoff`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct OnStartTakeoff {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
