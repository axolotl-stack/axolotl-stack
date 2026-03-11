use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorStalkAndPounceOnTargetPriority {}
impl Default for BehaviorStalkAndPounceOnTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.stalk_and_pounce_on_target`. Allows an entity to stalk a specific target. Once within range of the target, the entity will then leap at the target and deal damage based upon its attack attribute.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStalkAndPounceOnTarget {
    ///The amount of time the mob will be interested before pouncing. This happens when the mob is within range of pouncing
    pub interest_time: Option<f32>,
    ///The distance in blocks the mob jumps in the direction of their target.
    pub leap_dist: Option<f32>,
    ///The distance in blocks the mob jumps in the direction of its target.
    pub leap_distance: Option<f32>,
    ///The height in blocks the mob jumps when leaping at its target.
    pub leap_height: Option<f32>,
    ///The maximum distance away a target can be before the mob gives up on stalking.
    pub max_stalk_dist: Option<f32>,
    ///The maximum distance away from the target in blocks to begin pouncing at the target.
    pub pounce_max_dist: Option<f32>,
    ///priority
    pub priority: Option<BehaviorStalkAndPounceOnTargetPriority>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///The movement speed in which you stalk your target.
    pub stalk_speed: Option<f32>,
    ///The Maximum distance away from the target when landing from the pounce that will still result in damaging the target.
    pub strike_dist: Option<f32>,
    ///Filters to apply on the block the mob lands on to determine if it is valid for getting stuck.
    pub stuck_blocks: Option<crate::types::BedrockValue>,
    ///The amount of time the mob will be stuck if they fail and land on a block they can be stuck on.
    pub stuck_time: Option<f32>,
}
impl Default for BehaviorStalkAndPounceOnTarget {
    fn default() -> Self {
        Self {
            interest_time: Some(2f32),
            leap_dist: None,
            leap_distance: Some(0.8f32),
            leap_height: Some(0.9f32),
            max_stalk_dist: Some(10f32),
            pounce_max_dist: Some(5f32),
            priority: None,
            set_persistent: Some(false),
            stalk_speed: Some(1.2f32),
            strike_dist: Some(2f32),
            stuck_blocks: None,
            stuck_time: Some(2f32),
        }
    }
}
