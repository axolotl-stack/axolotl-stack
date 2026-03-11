use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement.sway`. This move control causes the mob to sway side to side giving the impression it is swimming.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementSway {
    ///The maximum number in degrees the mob can turn per tick.
    pub max_turn: Option<f32>,
    ///Strength of the sway movement.
    pub sway_amplitude: Option<f32>,
    ///Multiplier for the frequency of the sway movement.
    pub sway_frequency: Option<f32>,
}
impl Default for MovementSway {
    fn default() -> Self {
        Self {
            max_turn: Some(30f32),
            sway_amplitude: Some(0.05f32),
            sway_frequency: Some(0.5f32),
        }
    }
}
