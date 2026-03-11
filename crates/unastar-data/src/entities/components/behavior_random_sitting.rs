use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.random_sitting`. Allows the mob to randomly sit for a duration.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomSitting {
    /// cooldown
    pub cooldown: Option<f32>,
    /// cooldown_time
    pub cooldown_time: Option<f32>,
    /// min_sit_time
    pub min_sit_time: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// start_chance
    pub start_chance: Option<f32>,
    /// stop_chance
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
