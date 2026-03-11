use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.flee_sun`. Allows the mob to run away from direct sunlight and seek shade.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFleeSun {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorFleeSun {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(1f32),
        }
    }
}
