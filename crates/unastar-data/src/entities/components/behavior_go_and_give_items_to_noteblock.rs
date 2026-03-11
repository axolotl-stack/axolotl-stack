use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorGoAndGiveItemsToNoteblockPriority {}
impl Default for BehaviorGoAndGiveItemsToNoteblockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.go_and_give_items_to_noteblock`. [EXPERIMENTAL BEHAVIOR] The entity will attempt to toss the items from its inventory to a nearby recently played noteblock.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorGoAndGiveItemsToNoteblock {
    ///Sets the time an entity should continue delivering items to a noteblock after hearing it.
    pub listen_time: Option<i32>,
    ///Event(s) to run when this mob throws items.
    pub on_item_throw: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorGoAndGiveItemsToNoteblockPriority>,
    ///Sets the desired distance to be reached before throwing the items towards the block.
    pub reach_block_distance: Option<f32>,
    ///Sets the entity's speed when running toward the block.
    pub run_speed: Option<f32>,
    ///Sets the throw force.
    pub throw_force: Option<f32>,
    ///Sound to play when this mob throws an item.
    pub throw_sound: Option<String>,
    ///Sets the vertical throw multiplier that is applied on top of the throw force in the vertical direction.
    pub vertical_throw_mul: Option<f32>,
}
impl Default for BehaviorGoAndGiveItemsToNoteblock {
    fn default() -> Self {
        Self {
            listen_time: Some(30i32),
            on_item_throw: None,
            priority: None,
            reach_block_distance: Some(3f32),
            run_speed: Some(1f32),
            throw_force: Some(0.2f32),
            throw_sound: None,
            vertical_throw_mul: Some(1.5f32),
        }
    }
}
