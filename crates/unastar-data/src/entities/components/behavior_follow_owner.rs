use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.follow_owner`. Allows the mob to follow the player that owns them.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFollowOwner {
    /// can_teleport
    pub can_teleport: Option<bool>,
    /// ignore_vibration
    pub ignore_vibration: Option<bool>,
    /// max_distance
    pub max_distance: Option<f32>,
    /// post_teleport_distance
    pub post_teleport_distance: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// start_distance
    pub start_distance: Option<f32>,
    /// stop_distance
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorFollowOwner {
    fn default() -> Self {
        Self {
            can_teleport: Some(true),
            ignore_vibration: Some(true),
            max_distance: Some(60f32),
            post_teleport_distance: Some(0.0),
            priority: None,
            speed_multiplier: Some(1f32),
            start_distance: Some(10f32),
            stop_distance: Some(2f32),
        }
    }
}
