use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:physics`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Physics {
    /// has_gravity
    pub has_gravity: bool,
    /// has_collision
    pub has_collision: bool,
}
impl Default for Physics {
    fn default() -> Self {
        Self {
            has_gravity: true,
            has_collision: true,
        }
    }
}
