use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:vibration_damper`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct VibrationDamper {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
