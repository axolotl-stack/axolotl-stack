use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorLeapAtTargetPriority {}
impl Default for BehaviorLeapAtTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.leap_at_target`. Allows monsters to jump at and attack their target. Can only be used by hostile mobs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorLeapAtTarget {
    ///If true, the mob will only jump at its target if its on the ground. Setting it to false will allow it to jump even if its already in the air
    pub must_be_on_ground: Option<bool>,
    ///priority
    pub priority: Option<BehaviorLeapAtTargetPriority>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///Distance in blocks the mob jumps when leaping at its target.
    pub target_dist: Option<f32>,
    ///The height in blocks the mob jumps when leaping at its target.
    pub yd: Option<f32>,
}
impl Default for BehaviorLeapAtTarget {
    fn default() -> Self {
        Self {
            must_be_on_ground: Some(true),
            priority: None,
            set_persistent: Some(false),
            target_dist: None,
            yd: Some(0f32),
        }
    }
}
