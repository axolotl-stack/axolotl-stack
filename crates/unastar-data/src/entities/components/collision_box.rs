use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:collision_box`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct CollisionBox {
    /// width
    pub width: f32,
    /// height
    pub height: f32,
}
impl Default for CollisionBox {
    fn default() -> Self {
        Self {
            width: 0.6f32,
            height: 1.8f32,
        }
    }
}
