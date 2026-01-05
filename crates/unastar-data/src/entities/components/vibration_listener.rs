use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:vibration_listener`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct VibrationListener {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
