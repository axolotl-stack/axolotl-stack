use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPlayerRideTamedPriority {}
impl Default for BehaviorPlayerRideTamedPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.player_ride_tamed`. Allows the mob to be ridden by the player after being tamed.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlayerRideTamed {
    ///priority
    pub priority: Option<BehaviorPlayerRideTamedPriority>,
}
impl Default for BehaviorPlayerRideTamed {
    fn default() -> Self {
        Self { priority: None }
    }
}
