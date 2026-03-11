use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.hide`. Allows a mob with the hide component to attempt to move to - and hide at - an owned or nearby POI.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorHide {
    /// duration
    pub duration: Option<f32>,
    /// poi_type
    pub poi_type: Option<String>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// timeout_cooldown
    pub timeout_cooldown: Option<f32>,
}
impl Default for BehaviorHide {
    fn default() -> Self {
        Self {
            duration: Some(1f32),
            poi_type: None,
            priority: None,
            speed_multiplier: Some(1f32),
            timeout_cooldown: Some(8f32),
        }
    }
}
