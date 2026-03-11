use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.hover`. This move control causes the mob to hover.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementHover {
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
}
impl Default for MovementHover {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
        }
    }
}
