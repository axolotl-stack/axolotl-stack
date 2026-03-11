use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorBreedPriority {}
impl Default for BehaviorBreedPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorBreedSpeedMultiplier {}
impl Default for BehaviorBreedSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.breed`. Allows this mob to breed with other mobs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorBreed {
    ///priority
    pub priority: Option<BehaviorBreedPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorBreedSpeedMultiplier>,
}
impl Default for BehaviorBreed {
    fn default() -> Self {
        Self {
            priority: Some(BehaviorBreedPriority {}),
            speed_multiplier: Some(BehaviorBreedSpeedMultiplier {}),
        }
    }
}
