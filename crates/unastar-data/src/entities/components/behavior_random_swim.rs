use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomSwimPriority {}
impl Default for BehaviorRandomSwimPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomSwimSpeedMultiplier {}
impl Default for BehaviorRandomSwimSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_swim`. Allows an entity to randomly move through water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomSwim {
    ///If true, the mob will avoid surface water blocks by swimming below them.
    pub avoid_surface: Option<bool>,
    ///A random value to determine when to randomly move somewhere. This has a 1/interval chance to choose this goal
    pub interval: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRandomSwimPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRandomSwimSpeedMultiplier>,
    ///Distance in blocks on ground that the mob will look for a new spot to move to. Must be at least 1
    pub xz_dist: Option<i32>,
    ///Distance in blocks that the mob will look up or down for a new spot to move to. Must be at least 1
    pub y_dist: Option<i32>,
}
impl Default for BehaviorRandomSwim {
    fn default() -> Self {
        Self {
            avoid_surface: Some(true),
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {}),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
