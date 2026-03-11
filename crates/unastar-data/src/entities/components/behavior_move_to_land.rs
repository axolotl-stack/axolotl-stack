use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_to_land`. Allows the mob to move back onto land when in water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveToLand {
    /// goal_radius
    pub goal_radius: Option<f32>,
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
impl Default for BehaviorMoveToLand {
    fn default() -> Self {
        Self {
            goal_radius: Some(0.5f32),
            priority: None,
            search_count: Some(10i32),
            search_height: Some(1i32),
            search_range: Some(0i32),
            speed_multiplier: Some(1f32),
        }
    }
}
