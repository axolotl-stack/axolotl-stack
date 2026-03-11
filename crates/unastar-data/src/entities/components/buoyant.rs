use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:buoyant`. Enables an entity to float on the specified liquid blocks.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Buoyant {
    ///Applies gravity each tick. Causes more of a wave simulation, but will cause more gravity to be applied outside liquids.
    pub apply_gravity: Option<bool>,
    ///Base buoyancy used to calculate how much will a mob float.
    pub base_buoyancy: Option<f32>,
    ///Probability for a big wave hitting the entity. Only used if `simulate_waves` is true.
    pub big_wave_probability: Option<f32>,
    ///Multiplier for the speed to make a big wave. Triggered depending on `big_wave_probability`.
    pub big_wave_speed: Option<f32>,
    ///Base buoyancy used to calculate how much will a mob float.
    pub buoyancy: Option<f32>,
    ///How much an actor will be dragged down when the Buoyancy Component is removed.
    pub drag_down_on_buoyancy_removed: Option<f32>,
    ///List of blocks this entity can float on. Must be a liquid block.
    pub liquid_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Should the movement simulate waves going through.
    pub simulate_waves: Option<bool>,
}
impl Default for Buoyant {
    fn default() -> Self {
        Self {
            apply_gravity: Some(true),
            base_buoyancy: Some(1f32),
            big_wave_probability: Some(0.03f32),
            big_wave_speed: Some(10f32),
            buoyancy: None,
            drag_down_on_buoyancy_removed: Some(0f32),
            liquid_blocks: None,
            simulate_waves: Some(true),
        }
    }
}
