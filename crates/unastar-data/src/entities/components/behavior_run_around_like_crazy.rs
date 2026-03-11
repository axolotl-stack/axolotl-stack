use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.run_around_like_crazy`. Allows the mob to run around aimlessly.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRunAroundLikeCrazy {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorRunAroundLikeCrazy {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(1f32),
        }
    }
}
