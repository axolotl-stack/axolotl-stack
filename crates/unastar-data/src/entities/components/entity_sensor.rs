use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:entity_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct EntitySensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
