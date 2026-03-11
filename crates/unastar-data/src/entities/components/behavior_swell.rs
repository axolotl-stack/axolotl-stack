use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwellPriority {}
impl Default for BehaviorSwellPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swell`. Allows the creeper to swell up when a player is nearby. It can only be used by Creepers.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwell {
    ///priority
    pub priority: Option<BehaviorSwellPriority>,
    ///This mob starts swelling when a target is at least this many blocks away.
    pub start_distance: Option<f32>,
    ///This mob stops swelling when a target has moved away at least this many blocks.
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
