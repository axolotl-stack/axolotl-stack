use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsHomeRestrictionPriority {}
impl Default for BehaviorMoveTowardsHomeRestrictionPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {}
impl Default for BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_towards_home_restriction`. Allows mobs with the home component to move toward their pre-defined area that the mob should be restricted to.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsHomeRestriction {
    ///priority
    pub priority: Option<BehaviorMoveTowardsHomeRestrictionPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveTowardsHomeRestrictionSpeedMultiplier>,
}
impl Default for BehaviorMoveTowardsHomeRestriction {
    fn default() -> Self {
        Self {
            priority: Some(BehaviorMoveTowardsHomeRestrictionPriority {}),
            speed_multiplier: Some(BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {}),
        }
    }
}
