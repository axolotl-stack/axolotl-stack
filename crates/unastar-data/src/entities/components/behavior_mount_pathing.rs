use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMountPathingPriority {}
impl Default for BehaviorMountPathingPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMountPathingSpeedMultiplier {}
impl Default for BehaviorMountPathingSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.mount_pathing`. Allows the mob to move around on its own while mounted seeking a target to attack.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorMountPathing {
    ///priority
    pub priority: Option<BehaviorMountPathingPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMountPathingSpeedMultiplier>,
    ///The distance at which this mob wants to be away from its target.
    pub target_dist: Option<f32>,
    ///If true, this mob will chase after the target as long as it's a valid target.
    pub track_target: Option<bool>,
}
impl Default for BehaviorMountPathing {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {}),
            target_dist: Some(0f32),
            track_target: Some(false),
        }
    }
}
