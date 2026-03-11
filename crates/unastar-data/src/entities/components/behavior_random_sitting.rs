use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomSittingPriority {}
impl Default for BehaviorRandomSittingPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomSittingSpeedMultiplier {}
impl Default for BehaviorRandomSittingSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_sitting`. Allows the mob to randomly sit for a duration.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomSitting {
    ///Time in seconds the mob has to wait before using the goal again.
    pub cooldown: Option<f32>,
    ///Time in seconds the mob has to wait before using the goal again.
    pub cooldown_time: Option<f32>,
    ///The minimum amount of time in seconds before the mob can stand back up.
    pub min_sit_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorRandomSittingPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRandomSittingSpeedMultiplier>,
    ///This is the chance that the mob will start this goal, from 0 to 1.
    pub start_chance: Option<f32>,
    ///This is the chance that the mob will stop this goal, from 0 to 1.
    pub stop_chance: Option<f32>,
}
impl Default for BehaviorRandomSitting {
    fn default() -> Self {
        Self {
            cooldown: None,
            cooldown_time: Some(0f32),
            min_sit_time: Some(10f32),
            priority: None,
            speed_multiplier: None,
            start_chance: Some(0.1f32),
            stop_chance: Some(0.3f32),
        }
    }
}
