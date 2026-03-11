use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsTargetPriority {}
impl Default for BehaviorMoveTowardsTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsTargetSpeedMultiplier {}
impl Default for BehaviorMoveTowardsTargetSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_towards_target`. Allows mob to move towards its current target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsTarget {
    ///priority
    pub priority: Option<BehaviorMoveTowardsTargetPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveTowardsTargetSpeedMultiplier>,
    ///Defines the radius in blocks that the mob tries to be from the target. A value of 0 means it tries to occupy the same block as the target
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
