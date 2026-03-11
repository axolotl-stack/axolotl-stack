use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:jump.static`. Gives the entity the ability to jump.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct JumpStatic {
    /// jump_power
    pub jump_power: Option<f32>,
}
impl Default for JumpStatic {
    fn default() -> Self {
        Self {
            jump_power: Some(0.42f32),
        }
    }
}
