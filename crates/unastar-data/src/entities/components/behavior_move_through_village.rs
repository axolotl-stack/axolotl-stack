use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveThroughVillagePriority {}
impl Default for BehaviorMoveThroughVillagePriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveThroughVillageSpeedMultiplier {}
impl Default for BehaviorMoveThroughVillageSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_through_village`. Can only be used by Villagers. Allows the villagers to create paths around the village.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveThroughVillage {
    ///If true, the mob will only move through the village during night time.
    pub only_at_night: Option<bool>,
    ///priority
    pub priority: Option<BehaviorMoveThroughVillagePriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveThroughVillageSpeedMultiplier>,
}
impl Default for BehaviorMoveThroughVillage {
    fn default() -> Self {
        Self {
            only_at_night: Some(false),
            priority: None,
            speed_multiplier: Some(BehaviorMoveThroughVillageSpeedMultiplier {}),
        }
    }
}
