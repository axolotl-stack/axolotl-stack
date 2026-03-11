use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsDwellingRestrictionPriority {}
impl Default for BehaviorMoveTowardsDwellingRestrictionPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier {}
impl Default for BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_towards_dwelling_restriction`. Allows mobs with the dweller component to move toward their Village area that the mob should be restricted to.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsDwellingRestriction {
    ///priority
    pub priority: Option<BehaviorMoveTowardsDwellingRestrictionPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier>,
}
impl Default for BehaviorMoveTowardsDwellingRestriction {
    fn default() -> Self {
        Self {
            priority: Some(BehaviorMoveTowardsDwellingRestrictionPriority {}),
            speed_multiplier: Some(BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier {}),
        }
    }
}
