use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFindUnderwaterTreasurePriority {}
impl Default for BehaviorFindUnderwaterTreasurePriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFindUnderwaterTreasureSpeedMultiplier {}
impl Default for BehaviorFindUnderwaterTreasureSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.find_underwater_treasure`. Allows the mob to move towards the nearest underwater ruin or shipwreck.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFindUnderwaterTreasure {
    ///priority
    pub priority: Option<BehaviorFindUnderwaterTreasurePriority>,
    ///The range that the mob will search for a treasure chest within a ruin or shipwreck to move towards.
    pub search_range: Option<i32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorFindUnderwaterTreasureSpeedMultiplier>,
    ///The distance the mob will move before stopping.
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorFindUnderwaterTreasure {
    fn default() -> Self {
        Self {
            priority: None,
            search_range: Some(0i32),
            speed_multiplier: Some(BehaviorFindUnderwaterTreasureSpeedMultiplier {}),
            stop_distance: Some(2f32),
        }
    }
}
