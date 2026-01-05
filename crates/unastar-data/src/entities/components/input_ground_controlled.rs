use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:input_ground_controlled`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct InputGroundControlled {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
