use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.generic`. This move control allows a mob to fly, swim, climb, etc.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementGeneric {
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
}
impl Default for MovementGeneric {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
        }
    }
}
