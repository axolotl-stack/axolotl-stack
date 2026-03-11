use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:collision_box`. Sets the width and height of the Entity's collision box.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct CollisionBox {
    /// height
    pub height: Option<f32>,
    /// width
    pub width: Option<f32>,
}
impl Default for CollisionBox {
    fn default() -> Self {
        Self {
            height: Some(1.8f32),
            width: Some(0.6f32),
        }
    }
}
