use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeKeepOnJumpingControlFlags {}
impl Default for BehaviorSlimeKeepOnJumpingControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeKeepOnJumpingPriority {}
impl Default for BehaviorSlimeKeepOnJumpingPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeKeepOnJumpingSpeedMultiplier {}
impl Default for BehaviorSlimeKeepOnJumpingSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.slime_keep_on_jumping`. Can only be used by Slimes and Magma Cubes. Allows the mob to continuously jump around like a slime.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSlimeKeepOnJumping {
    ///control_flags
    pub control_flags: Option<BehaviorSlimeKeepOnJumpingControlFlags>,
    ///priority
    pub priority: Option<BehaviorSlimeKeepOnJumpingPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSlimeKeepOnJumpingSpeedMultiplier>,
}
impl Default for BehaviorSlimeKeepOnJumping {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSlimeKeepOnJumpingControlFlags {}),
            priority: Some(BehaviorSlimeKeepOnJumpingPriority {}),
            speed_multiplier: Some(BehaviorSlimeKeepOnJumpingSpeedMultiplier {}),
        }
    }
}
