use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:environment_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct EnvironmentSensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
