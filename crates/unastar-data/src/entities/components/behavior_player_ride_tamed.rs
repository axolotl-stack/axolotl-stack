use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.player_ride_tamed`. Allows the mob to be ridden by the player after being tamed.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlayerRideTamed {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorPlayerRideTamed {
    fn default() -> Self {
        Self { priority: None }
    }
}
