use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimIdleControlFlags {}
impl Default for BehaviorSwimIdleControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimIdlePriority {}
impl Default for BehaviorSwimIdlePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swim_idle`. Allows the entity go idle, if swimming. Entity must be in water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimIdle {
    ///control_flags
    pub control_flags: Option<BehaviorSwimIdleControlFlags>,
    ///Amount of time (in seconds) to stay idle.
    pub idle_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSwimIdlePriority>,
    ///Percent chance this entity will go idle, 1.0 = 100%.
    pub success_rate: Option<f32>,
}
impl Default for BehaviorSwimIdle {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSwimIdleControlFlags {}),
            idle_time: Some(5f32),
            priority: Some(BehaviorSwimIdlePriority {}),
            success_rate: Some(0.1f32),
        }
    }
}
