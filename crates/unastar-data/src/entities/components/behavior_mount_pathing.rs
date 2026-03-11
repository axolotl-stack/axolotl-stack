use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.mount_pathing`. Allows the mob to move around on its own while mounted seeking a target to attack.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorMountPathing {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// target_dist
    pub target_dist: Option<f32>,
    /// track_target
    pub track_target: Option<bool>,
}
impl Default for BehaviorMountPathing {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(1f32),
            target_dist: Some(0f32),
            track_target: Some(false),
        }
    }
}
