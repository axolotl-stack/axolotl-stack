use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:block_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BlockSensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
