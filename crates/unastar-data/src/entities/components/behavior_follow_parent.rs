use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFollowParentPriority {}
impl Default for BehaviorFollowParentPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFollowParentSpeedMultiplier {}
impl Default for BehaviorFollowParentSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.follow_parent`. Allows the mob to follow their parent around.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFollowParent {
    ///priority
    pub priority: Option<BehaviorFollowParentPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorFollowParentSpeedMultiplier>,
}
impl Default for BehaviorFollowParent {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(BehaviorFollowParentSpeedMultiplier {}),
        }
    }
}
