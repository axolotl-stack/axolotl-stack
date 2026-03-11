use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorHidePriority {}
impl Default for BehaviorHidePriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorHideSpeedMultiplier {}
impl Default for BehaviorHideSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.hide`. Allows a mob with the hide component to attempt to move to - and hide at - an owned or nearby POI.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorHide {
    ///Amount of time in seconds that the mob reacts.
    pub duration: Option<f32>,
    ///Defines what POI type to hide at.
    pub poi_type: Option<String>,
    ///priority
    pub priority: Option<BehaviorHidePriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorHideSpeedMultiplier>,
    ///The cooldown time in seconds before the goal can be reused after a internal failure or timeout condition.
    pub timeout_cooldown: Option<f32>,
}
impl Default for BehaviorHide {
    fn default() -> Self {
        Self {
            duration: Some(1f32),
            poi_type: None,
            priority: None,
            speed_multiplier: Some(BehaviorHideSpeedMultiplier {}),
            timeout_cooldown: Some(8f32),
        }
    }
}
