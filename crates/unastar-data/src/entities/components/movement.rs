use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:movement`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Movement {
    /// value
    pub speed: f32,
}
impl Default for Movement {
    fn default() -> Self {
        Self { speed: 0.25f32 }
    }
}
