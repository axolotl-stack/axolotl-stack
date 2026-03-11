use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFleeSunPriority {}
impl Default for BehaviorFleeSunPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFleeSunSpeedMultiplier {}
impl Default for BehaviorFleeSunSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.flee_sun`. Allows the mob to run away from direct sunlight and seek shade.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFleeSun {
    ///priority
    pub priority: Option<BehaviorFleeSunPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorFleeSunSpeedMultiplier>,
}
impl Default for BehaviorFleeSun {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(BehaviorFleeSunSpeedMultiplier {}),
        }
    }
}
