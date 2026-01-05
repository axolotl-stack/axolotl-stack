use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:damage_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct DamageSensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
