use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:horse.jump_strength`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct HorseJumpStrength {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
