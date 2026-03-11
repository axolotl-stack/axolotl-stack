use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTakeBlockPriority {}
impl Default for BehaviorTakeBlockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.take_block`. Allows an entity to take blocks from the world.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTakeBlock {
    ///If true, whether the goal is affected by the mob griefing game rule.
    pub affected_by_griefing_rule: Option<bool>,
    ///Block descriptors for which blocks are valid to be taken by the entity, if empty all blocks are valid.
    pub blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Filters for if the entity should try to take a block. Self and Target are set.
    pub can_take: Option<crate::types::BedrockValue>,
    ///Chance each tick for the entity to try and take a block.
    pub chance: Option<f32>,
    ///Trigger ran if the entity does take a block. Self, Target, and Block are set.
    pub on_take: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorTakeBlockPriority>,
    ///If true, whether the entity needs line of sight to the block they are trying to take.
    pub requires_line_of_sight: Option<bool>,
    ///XZ range from which the entity will try and take blocks from.
    pub xz_range: Option<crate::types::RangeOrVal<f32>>,
    ///Y range from which the entity will try and take blocks from.
    pub y_range: Option<crate::types::RangeOrVal<f32>>,
}
impl Default for BehaviorTakeBlock {
    fn default() -> Self {
        Self {
            affected_by_griefing_rule: None,
            blocks: None,
            can_take: None,
            chance: None,
            on_take: None,
            priority: None,
            requires_line_of_sight: None,
            xz_range: None,
            y_range: None,
        }
    }
}
