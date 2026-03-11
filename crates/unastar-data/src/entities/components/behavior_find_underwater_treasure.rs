use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.find_underwater_treasure`. Allows the mob to move towards the nearest underwater ruin or shipwreck.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFindUnderwaterTreasure {
    /// priority
    pub priority: Option<i32>,
    /// search_range
    pub search_range: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// stop_distance
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorFindUnderwaterTreasure {
    fn default() -> Self {
        Self {
            priority: None,
            search_range: Some(0i32),
            speed_multiplier: Some(1f32),
            stop_distance: Some(2f32),
        }
    }
}
