use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.leap_at_target`. Allows monsters to jump at and attack their target. Can only be used by hostile mobs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorLeapAtTarget {
    /// must_be_on_ground
    pub must_be_on_ground: Option<bool>,
    /// priority
    pub priority: Option<i32>,
    /// set_persistent
    pub set_persistent: Option<bool>,
    /// target_dist
    pub target_dist: Option<f32>,
    /// yd
    pub yd: Option<f32>,
}
impl Default for BehaviorLeapAtTarget {
    fn default() -> Self {
        Self {
            must_be_on_ground: Some(true),
            priority: None,
            set_persistent: Some(false),
            target_dist: None,
            yd: Some(0f32),
        }
    }
}
