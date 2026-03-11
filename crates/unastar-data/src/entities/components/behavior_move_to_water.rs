use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveToWaterPriority {}
impl Default for BehaviorMoveToWaterPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveToWaterSpeedMultiplier {}
impl Default for BehaviorMoveToWaterSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_to_water`. Allows the mob to move back into water when on land.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveToWater {
    ///Distance in blocks within the mob considers it has reached the goal. This is the `wiggle room` to stop the AI from bouncing back and forth trying to reach a specific spot
    pub goal_radius: Option<f32>,
    ///priority
    pub priority: Option<BehaviorMoveToWaterPriority>,
    ///The number of blocks each tick that the mob will check within it's search range and height for a valid block to move to. A value of 0 will have the mob check every block within range in one tick
    pub search_count: Option<i32>,
    ///Height in blocks the mob will look for water to move towards.
    pub search_height: Option<i32>,
    ///The distance in blocks it will look for water to move towards.
    pub search_range: Option<i32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveToWaterSpeedMultiplier>,
}
impl Default for BehaviorMoveToWater {
    fn default() -> Self {
        Self {
            goal_radius: Some(0.5f32),
            priority: None,
            search_count: Some(10i32),
            search_height: Some(1i32),
            search_range: Some(0i32),
            speed_multiplier: Some(BehaviorMoveToWaterSpeedMultiplier {}),
        }
    }
}
