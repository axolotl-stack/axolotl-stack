use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_towards_target`. Allows mob to move towards its current target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsTarget {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// within_radius
    pub within_radius: Option<f32>,
}
impl Default for BehaviorMoveTowardsTarget {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: None,
            within_radius: Some(0f32),
        }
    }
}
