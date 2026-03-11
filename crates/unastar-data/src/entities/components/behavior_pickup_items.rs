use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPickupItemsPriority {}
impl Default for BehaviorPickupItemsPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPickupItemsSpeedMultiplier {}
impl Default for BehaviorPickupItemsSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.pickup_items`. Allows the mob to pick up items on the ground.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorPickupItems {
    ///If true, the mob can pickup any item.
    pub can_pickup_any_item: Option<bool>,
    ///If true, the mob can pickup items to its hand or armor slots.
    pub can_pickup_to_hand_or_equipment: Option<bool>,
    ///Amount of time an offended entity needs before being willing to pick up items.
    pub cooldown_after_being_attacked: Option<f32>,
    ///List of items this mob will not pick up.
    pub excluded_items: Option<Vec<crate::types::BedrockValue>>,
    ///Distance in blocks within the mob considers it has reached the goal. This is the `wiggle room` to stop the AI from bouncing back and forth trying to reach a specific spot.
    pub goal_radius: Option<f32>,
    ///Maximum distance this mob will look for items to pick up.
    pub max_dist: Option<f32>,
    ///If true, depending on the difficulty, there is a random chance that the mob may not be able to pickup items.
    pub pickup_based_on_chance: Option<bool>,
    ///If true, the mob will pickup the same item as the item in its hand.
    pub pickup_same_items_as_in_hand: Option<bool>,
    ///priority
    pub priority: Option<BehaviorPickupItemsPriority>,
    ///Height in blocks the mob will look for items to pick up.
    pub search_height: Option<f32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorPickupItemsSpeedMultiplier>,
    ///If true, this mob will chase after the target as long as it's a valid target.
    pub track_target: Option<bool>,
}
impl Default for BehaviorPickupItems {
    fn default() -> Self {
        Self {
            can_pickup_any_item: Some(false),
            can_pickup_to_hand_or_equipment: Some(true),
            cooldown_after_being_attacked: None,
            excluded_items: None,
            goal_radius: Some(0.5f32),
            max_dist: Some(0f32),
            pickup_based_on_chance: Some(false),
            pickup_same_items_as_in_hand: None,
            priority: None,
            search_height: None,
            speed_multiplier: Some(BehaviorPickupItemsSpeedMultiplier {}),
            track_target: Some(false),
        }
    }
}
