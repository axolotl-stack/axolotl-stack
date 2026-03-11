use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomStrollPriority {}
impl Default for BehaviorRandomStrollPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomStrollSpeedMultiplier {}
impl Default for BehaviorRandomStrollSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_stroll`. Allows a mob to randomly stroll around.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorRandomStroll {
    ///A random value to determine when to randomly move somewhere. This has a 1/interval chance to choose this goal
    pub interval: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRandomStrollPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRandomStrollSpeedMultiplier>,
    ///Distance in blocks on ground that the mob will look for a new spot to move to. Must be at least 1
    pub xz_dist: Option<i32>,
    ///Distance in blocks that the mob will look up or down for a new spot to move to. Must be at least 1
    pub y_dist: Option<i32>,
}
impl Default for BehaviorRandomStroll {
    fn default() -> Self {
        Self {
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
