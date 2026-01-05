use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:target_nearby_sensor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct TargetNearbySensor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
