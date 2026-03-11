use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.basic`. defines the movement of an entity.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct MovementBasic {
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
}
impl Default for MovementBasic {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
        }
    }
}
