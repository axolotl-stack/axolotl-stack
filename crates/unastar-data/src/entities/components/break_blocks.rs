use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:break_blocks`. Specifies the blocks that this entity can break as it moves around.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BreakBlocks {
    ///A list of the blocks that can be broken as this entity moves around.
    pub breakable_blocks: Option<Vec<String>>,
}
impl Default for BreakBlocks {
    fn default() -> Self {
        Self {
            breakable_blocks: None,
        }
    }
}
