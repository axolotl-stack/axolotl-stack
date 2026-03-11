use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.fly`. This move control causes the mob to fly.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementFly {
    /// max_turn
    pub max_turn: Option<f32>,
}
impl Default for MovementFly {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
        }
    }
}
