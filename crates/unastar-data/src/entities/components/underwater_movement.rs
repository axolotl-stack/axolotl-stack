use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:underwater_movement`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct UnderwaterMovement {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
