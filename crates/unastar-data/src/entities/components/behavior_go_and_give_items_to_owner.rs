use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorGoAndGiveItemsToOwnerPriority {}
impl Default for BehaviorGoAndGiveItemsToOwnerPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.go_and_give_items_to_owner`. [EXPERIMENTAL BEHAVIOR] The entity will attempt to toss the items from its inventory to its owner.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorGoAndGiveItemsToOwner {
    ///Event(s) to run when this mob throws items.
    pub on_item_throw: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorGoAndGiveItemsToOwnerPriority>,
    ///Sets the desired distance to be reached before giving items to owner.
    pub reach_mob_distance: Option<f32>,
    ///Sets the entity's speed when running toward the owner.
    pub run_speed: Option<f32>,
    ///Sets the throw force.
    pub throw_force: Option<f32>,
    ///Sound to play when this mob throws an item.
    pub throw_sound: Option<String>,
    ///Sets the vertical throw multiplier that is applied on top of the throw force in the vertical direction.
    pub vertical_throw_mul: Option<f32>,
}
impl Default for BehaviorGoAndGiveItemsToOwner {
    fn default() -> Self {
        Self {
            on_item_throw: None,
            priority: None,
            reach_mob_distance: Some(3f32),
            run_speed: Some(1f32),
            throw_force: Some(0.2f32),
            throw_sound: Some("item_thrown".to_string()),
            vertical_throw_mul: Some(1.5f32),
        }
    }
}
