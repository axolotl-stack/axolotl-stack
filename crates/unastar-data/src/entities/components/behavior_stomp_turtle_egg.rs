use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorStompTurtleEggPriority {}
impl Default for BehaviorStompTurtleEggPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorStompTurtleEggSpeedMultiplier {}
impl Default for BehaviorStompTurtleEggSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.stomp_turtle_egg`. Allows this mob to stomp turtle eggs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStompTurtleEgg {
    ///Distance in blocks within the mob considers it has reached the goal. This is the `wiggle room` to stop the AI from bouncing back and forth trying to reach a specific spot
    pub goal_radius: Option<f32>,
    ///A random value to determine when to randomly move somewhere. This has a 1/interval chance to choose this goal
    pub interval: Option<i32>,
    ///priority
    pub priority: Option<BehaviorStompTurtleEggPriority>,
    ///The number of blocks each tick that the mob will check within it's search range and height for a valid block to move to. A value of 0 will have the mob check every block within range in one tick
    pub search_count: Option<i32>,
    ///Height in blocks the mob will look for turtle eggs to move towards.
    pub search_height: Option<i32>,
    ///The distance in blocks it will look for turtle eggs to move towards.
    pub search_range: Option<i32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorStompTurtleEggSpeedMultiplier>,
}
impl Default for BehaviorStompTurtleEgg {
    fn default() -> Self {
        Self {
            goal_radius: Some(0.5f32),
            interval: Some(120i32),
            priority: None,
            search_count: None,
            search_height: Some(1i32),
            search_range: Some(0i32),
            speed_multiplier: Some(BehaviorStompTurtleEggSpeedMultiplier {}),
        }
    }
}
