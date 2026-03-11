use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.swell`. Allows the creeper to swell up when a player is nearby. It can only be used by Creepers.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwell {
    /// priority
    pub priority: Option<i32>,
    /// start_distance
    pub start_distance: Option<f32>,
    /// stop_distance
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorSwell {
    fn default() -> Self {
        Self {
            priority: None,
            start_distance: Some(10f32),
            stop_distance: Some(2f32),
        }
    }
}
