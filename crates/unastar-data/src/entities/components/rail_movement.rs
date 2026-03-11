use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:rail_movement`. Defines the entity's movement on the rails. An entity with this component is only allowed to move on the rail.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct RailMovement {
    /// max_speed
    pub max_speed: Option<f32>,
}
impl Default for RailMovement {
    fn default() -> Self {
        Self {
            max_speed: Some(0.4f32),
        }
    }
}
