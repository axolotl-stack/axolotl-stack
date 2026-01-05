use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:rotation_locked_to_vehicle`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct RotationLockedToVehicle {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
