use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorJumpToBlockPriority {}
impl Default for BehaviorJumpToBlockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.jump_to_block`. Allows an entity to jump to another random block.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorJumpToBlock {
    ///Minimum and maximum cooldown time-range (positive, in seconds) between each attempted jump.
    pub cooldown_range: Option<Vec<f32>>,
    ///Blocks that the mob can't jump to.
    pub forbidden_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///The maximum velocity with which the mob can jump.
    pub max_velocity: Option<f32>,
    ///The minimum distance (in blocks) from the mob to a block, in order to consider jumping to it.
    pub minimum_distance: Option<i32>,
    ///The minimum length (in blocks) of the mobs path to a block, in order to consider jumping to it.
    pub minimum_path_length: Option<i32>,
    ///Blocks that the mob prefers jumping to.
    pub preferred_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Chance (between 0.0 and 1.0) that the mob will jump to a preferred block, if in range. Only matters if preferred blocks are defined.
    pub preferred_blocks_chance: Option<f32>,
    ///priority
    pub priority: Option<BehaviorJumpToBlockPriority>,
    ///The scalefactor of the bounding box of the mob while it is jumping.
    pub scale_factor: Option<f32>,
    ///The height (in blocks, in range [2, 15]) of the search box, centered around the mob.
    pub search_height: Option<i32>,
    ///The width (in blocks, in range [2, 15]) of the search box, centered around the mob.
    pub search_width: Option<i32>,
}
impl Default for BehaviorJumpToBlock {
    fn default() -> Self {
        Self {
            cooldown_range: None,
            forbidden_blocks: None,
            max_velocity: Some(1.5f32),
            minimum_distance: Some(2i32),
            minimum_path_length: Some(5i32),
            preferred_blocks: None,
            preferred_blocks_chance: Some(1f32),
            priority: None,
            scale_factor: Some(0.7f32),
            search_height: Some(10i32),
            search_width: Some(8i32),
        }
    }
}
