use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.random_breach`. Allows the mob to randomly break surface of the water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomBreach {
    /// cooldown_time
    pub cooldown_time: Option<f32>,
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
impl Default for BehaviorRandomBreach {
    fn default() -> Self {
        Self {
            cooldown_time: Some(0f32),
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(1f32),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
