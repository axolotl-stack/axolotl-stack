use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct PreferredPathPreferredPathBlocks {
    ///blocks
    pub blocks: Option<Vec<crate::types::BedrockValue>>,
    ///cost
    pub cost: Option<f32>,
}
impl Default for PreferredPathPreferredPathBlocks {
    fn default() -> Self {
        Self {
            blocks: None,
            cost: None,
        }
    }
}
/// Bedrock component `minecraft:preferred_path`. Specifies costing information for mobs that prefer to walk on preferred paths.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PreferredPath {
    ///Cost for non-preferred blocks.
    pub default_block_cost: Option<f32>,
    ///Added cost for jumping up a node.
    pub jump_cost: Option<i32>,
    ///Distance mob can fall without taking damage.
    pub max_fall_blocks: Option<i32>,
    ///A list of blocks with their associated cost.
    pub preferred_path_blocks: Option<Vec<PreferredPathPreferredPathBlocks>>,
}
impl Default for PreferredPath {
    fn default() -> Self {
        Self {
            default_block_cost: Some(0f32),
            jump_cost: Some(0i32),
            max_fall_blocks: Some(3i32),
            preferred_path_blocks: None,
        }
    }
}
