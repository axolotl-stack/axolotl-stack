use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimWanderControlFlags {}
impl Default for BehaviorSwimWanderControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimWanderPriority {}
impl Default for BehaviorSwimWanderPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimWanderSpeedMultiplier {}
impl Default for BehaviorSwimWanderSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swim_wander`. Has the fish swim around when they can't pathfind.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimWander {
    ///control_flags
    pub control_flags: Option<BehaviorSwimWanderControlFlags>,
    ///Percent chance to start wandering, when not path-finding. 1 = 100%
    pub interval: Option<f32>,
    ///Distance to look ahead for obstacle avoidance, while wandering.
    pub look_ahead: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSwimWanderPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSwimWanderSpeedMultiplier>,
    ///Amount of time (in seconds) to wander after wandering behavior was successfully started.
    pub wander_time: Option<f32>,
}
impl Default for BehaviorSwimWander {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSwimWanderControlFlags {}),
            interval: Some(0.00833f32),
            look_ahead: Some(5f32),
            priority: Some(BehaviorSwimWanderPriority {}),
            speed_multiplier: Some(BehaviorSwimWanderSpeedMultiplier {}),
            wander_time: Some(5f32),
        }
    }
}
