use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRaidGardenPriority {}
impl Default for BehaviorRaidGardenPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRaidGardenSpeedMultiplier {}
impl Default for BehaviorRaidGardenSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.raid_garden`. Allows the mob to eat/raid crops out of farms until they are full.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRaidGarden {
    ///Blocks that the mob is looking for to eat.
    pub blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Time in seconds between each time it eats.
    pub eat_delay: Option<i32>,
    ///Amount of time in seconds before this mob wants to eat again.
    pub full_delay: Option<i32>,
    ///Distance in blocks within the mob considers it has reached the goal. This is the `wiggle room` to stop the AI from bouncing back and forth trying to reach a specific spot
    pub goal_radius: Option<f32>,
    ///Time in seconds before starting to eat/raid once it arrives at it.
    pub initial_eat_delay: Option<i32>,
    ///Maximum number of things this entity wants to eat.
    pub max_to_eat: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRaidGardenPriority>,
    ///Height in blocks the mob will look for crops to eat.
    pub search_height: Option<i32>,
    ///Distance in blocks the mob will look for crops to eat.
    pub search_range: Option<i32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRaidGardenSpeedMultiplier>,
}
impl Default for BehaviorRaidGarden {
    fn default() -> Self {
        Self {
            blocks: None,
            eat_delay: Some(2i32),
            full_delay: Some(100i32),
            goal_radius: Some(0.5f32),
            initial_eat_delay: Some(0i32),
            max_to_eat: Some(6i32),
            priority: None,
            search_height: None,
            search_range: Some(0i32),
            speed_multiplier: Some(BehaviorRaidGardenSpeedMultiplier {}),
        }
    }
}
