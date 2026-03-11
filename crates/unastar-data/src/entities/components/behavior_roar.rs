use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRoarControlFlags {}
impl Default for BehaviorRoarControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRoarPriority {}
impl Default for BehaviorRoarPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.roar`. Allows this entity to roar at another entity based on data in minecraft:anger_level. Once the anger threshold specified in minecraft:anger_level has been reached, this entity will roar for the specified amount of time, look at the other entity, apply anger boost towards it, and finally target it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRoar {
    ///control_flags
    pub control_flags: Option<BehaviorRoarControlFlags>,
    ///Goal duration in seconds.
    pub duration: Option<f32>,
    ///priority
    pub priority: Option<BehaviorRoarPriority>,
}
impl Default for BehaviorRoar {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorRoarControlFlags {}),
            duration: Some(0f32),
            priority: Some(BehaviorRoarPriority {}),
        }
    }
}
