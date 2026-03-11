use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.breed`. Allows this mob to breed with other mobs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorBreed {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorBreed {
    fn default() -> Self {
        Self {
            priority: Some(0i32),
            speed_multiplier: Some(1f32),
        }
    }
}
