use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomBreachPriority {}
impl Default for BehaviorRandomBreachPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomBreachSpeedMultiplier {}
impl Default for BehaviorRandomBreachSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_breach`. Allows the mob to randomly break surface of the water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomBreach {
    ///Time in seconds the mob has to wait before using the goal again.
    pub cooldown_time: Option<f32>,
    ///A random value to determine when to randomly move somewhere. This has a 1/interval chance to choose this goal
    pub interval: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRandomBreachPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRandomBreachSpeedMultiplier>,
    ///Distance in blocks on ground that the mob will look for a new spot to move to. Must be at least 1
    pub xz_dist: Option<i32>,
    ///Distance in blocks that the mob will look up or down for a new spot to move to. Must be at least 1
    pub y_dist: Option<i32>,
}
impl Default for BehaviorRandomBreach {
    fn default() -> Self {
        Self {
            cooldown_time: Some(0f32),
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(BehaviorRandomBreachSpeedMultiplier {}),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
