use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorEatBlockEatAndReplaceBlockPairs {
    ///The block to eat.
    pub eat_block: Option<String>,
    ///The block to replace the eaten block with.
    pub replace_block: Option<String>,
}
impl Default for BehaviorEatBlockEatAndReplaceBlockPairs {
    fn default() -> Self {
        Self {
            eat_block: None,
            replace_block: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorEatBlockPriority {}
impl Default for BehaviorEatBlockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.eat_block`. Allows the entity to consume a block, replace the eaten block with another block, and trigger an event as a result.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorEatBlock {
    ///A collection of pairs of blocks; the first ("eat_block")is the block the entity should eat, the second ("replace_block") is the block that should replace the eaten block.
    pub eat_and_replace_block_pairs: Option<Vec<BehaviorEatBlockEatAndReplaceBlockPairs>>,
    ///The event to trigger when the block eating animation has completed.
    pub on_eat: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorEatBlockPriority>,
    ///A molang expression defining the success chance the entity has to consume a block.
    pub success_chance: Option<crate::types::MolangOr<f32>>,
    ///The amount of time (in seconds) it takes for the block to be eaten upon a successful eat attempt.
    pub time_until_eat: Option<f32>,
}
impl Default for BehaviorEatBlock {
    fn default() -> Self {
        Self {
            eat_and_replace_block_pairs: None,
            on_eat: None,
            priority: None,
            success_chance: Some(crate::types::MolangOr::Expr("0.02".to_string())),
            time_until_eat: Some(1.8f32),
        }
    }
}
