use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement_sound_distance_offset`. Sets the offset used to determine the next step distance for playing a movement sound.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct MovementSoundDistanceOffset {
    /// value
    pub value: f32,
}
impl Default for MovementSoundDistanceOffset {
    fn default() -> Self {
        Self { value: 1f32 }
    }
}
