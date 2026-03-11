use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPlaceBlockPriority {}
impl Default for BehaviorPlaceBlockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.place_block`. Allows an entity to place blocks in the world
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlaceBlock {
    ///If true, whether the goal is affected by the mob griefing game rule.
    pub affected_by_griefing_rule: Option<bool>,
    ///Filters for if the entity should try to place its block. Self and Target are set.
    pub can_place: Option<crate::types::BedrockValue>,
    ///Chance each tick for the entity to try and place a block.
    pub chance: Option<f32>,
    ///Trigger ran if the entity does place its block. Self, Target, and Block are set.
    pub on_place: Option<crate::types::BedrockValue>,
    ///Block descriptors for which blocks are valid to be placed from the entity's carried item, if empty all blocks are valid.
    pub placeable_carried_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///priority
    pub priority: Option<BehaviorPlaceBlockPriority>,
    ///Weighted block descriptors for which blocks should be randomly placed, if empty the entity will try to place its carried block from placeable_carried_blocks.
    pub randomly_placeable_blocks: Option<Vec<Vec<crate::types::BedrockValue>>>,
    ///XZ range from which the entity will try and place blocks in.
    pub xz_range: Option<crate::types::RangeOrVal<f32>>,
    ///Y range from which the entity will try and place blocks in.
    pub y_range: Option<crate::types::RangeOrVal<f32>>,
}
impl Default for BehaviorPlaceBlock {
    fn default() -> Self {
        Self {
            affected_by_griefing_rule: None,
            can_place: None,
            chance: None,
            on_place: None,
            placeable_carried_blocks: None,
            priority: None,
            randomly_placeable_blocks: None,
            xz_range: None,
            y_range: None,
        }
    }
}
