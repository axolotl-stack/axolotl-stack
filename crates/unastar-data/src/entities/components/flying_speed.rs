use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:flying_speed`
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct FlyingSpeed {
    /// value
    pub speed: f32,
}
impl Default for FlyingSpeed {
    fn default() -> Self {
        Self { speed: 0.02f32 }
    }
}
