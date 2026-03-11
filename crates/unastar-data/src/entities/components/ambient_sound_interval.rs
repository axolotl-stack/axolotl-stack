use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:ambient_sound_interval`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct AmbientSoundInterval {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
