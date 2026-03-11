use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.random_swim`. Allows an entity to randomly move through water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomSwim {
    /// avoid_surface
    pub avoid_surface: Option<bool>,
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
impl Default for BehaviorRandomSwim {
    fn default() -> Self {
        Self {
            avoid_surface: Some(true),
            interval: Some(120i32),
            priority: None,
            speed_multiplier: Some(1f32),
            xz_dist: Some(10i32),
            y_dist: Some(7i32),
        }
    }
}
