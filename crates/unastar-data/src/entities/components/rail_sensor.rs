use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:rail_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct RailSensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
