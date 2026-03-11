use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRunAroundLikeCrazyPriority {}
impl Default for BehaviorRunAroundLikeCrazyPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRunAroundLikeCrazySpeedMultiplier {}
impl Default for BehaviorRunAroundLikeCrazySpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.run_around_like_crazy`. Allows the mob to run around aimlessly.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRunAroundLikeCrazy {
    ///priority
    pub priority: Option<BehaviorRunAroundLikeCrazyPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRunAroundLikeCrazySpeedMultiplier>,
}
impl Default for BehaviorRunAroundLikeCrazy {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(BehaviorRunAroundLikeCrazySpeedMultiplier {}),
        }
    }
}
