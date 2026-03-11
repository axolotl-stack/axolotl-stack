use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.jump`. Move control that causes the mob to jump as it moves with a specified delay between jumps.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementJump {
    ///Delay after landing when using the slime move control.
    pub jump_delay: Option<Vec<f32>>,
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
}
impl Default for MovementJump {
    fn default() -> Self {
        Self {
            jump_delay: None,
            max_turn: Some(30f32),
        }
    }
}
