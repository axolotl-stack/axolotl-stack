use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:water_movement`. Defines the speed with which an entity can move through water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct WaterMovement {
    /// drag_factor
    pub drag_factor: Option<f32>,
}
impl Default for WaterMovement {
    fn default() -> Self {
        Self {
            drag_factor: Some(0.8f32),
        }
    }
}
