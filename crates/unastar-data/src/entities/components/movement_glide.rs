use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.glide`. This is the move control for a flying mob that has a gliding movement.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementGlide {
    /// max_turn
    pub max_turn: Option<f32>,
    /// speed_when_turning
    pub speed_when_turning: Option<f32>,
    /// start_speed
    pub start_speed: Option<f32>,
}
impl Default for MovementGlide {
    fn default() -> Self {
        Self {
            max_turn: None,
            speed_when_turning: None,
            start_speed: None,
        }
    }
}
