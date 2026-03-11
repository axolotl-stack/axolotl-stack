use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.random_stroll`. Allows a mob to randomly stroll around.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorRandomStroll {
    /// interval
    pub interval: Option<i32>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// xz_dist
    pub xz_dist: Option<i32>,
    /// y_dist
    pub y_dist: Option<i32>,
}
impl Default for BehaviorRandomStroll {
    fn default() -> Self {
        Self {
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(1f32),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
