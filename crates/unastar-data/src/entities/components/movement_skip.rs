use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.skip`. This move control causes the mob to hop as it moves.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementSkip {
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
}
impl Default for MovementSkip {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
        }
    }
}
