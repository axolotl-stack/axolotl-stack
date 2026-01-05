use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement_sound_distance_offset`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementSoundDistanceOffset {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
