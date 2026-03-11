use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_towards_home_restriction`. Allows mobs with the home component to move toward their pre-defined area that the mob should be restricted to.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsHomeRestriction {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorMoveTowardsHomeRestriction {
    fn default() -> Self {
        Self {
            priority: Some(0i32),
            speed_multiplier: Some(1f32),
        }
    }
}
