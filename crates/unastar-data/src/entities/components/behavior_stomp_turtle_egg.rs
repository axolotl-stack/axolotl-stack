use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.stomp_turtle_egg`. Allows this mob to stomp turtle eggs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStompTurtleEgg {
    /// goal_radius
    pub goal_radius: Option<f32>,
    /// interval
    pub interval: Option<i32>,
    /// priority
    pub priority: Option<i32>,
    /// search_count
    pub search_count: Option<i32>,
    /// search_height
    pub search_height: Option<i32>,
    /// search_range
    pub search_range: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
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
            speed_multiplier: Some(1f32),
        }
    }
}
