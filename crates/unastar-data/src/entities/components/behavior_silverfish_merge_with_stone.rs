use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSilverfishMergeWithStonePriority {}
impl Default for BehaviorSilverfishMergeWithStonePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.silverfish_merge_with_stone`. Allows the mob to go into stone blocks like Silverfish do. Currently it can only be used by Silverfish.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSilverfishMergeWithStone {
    ///priority
    pub priority: Option<BehaviorSilverfishMergeWithStonePriority>,
}
impl Default for BehaviorSilverfishMergeWithStone {
    fn default() -> Self {
        Self { priority: None }
    }
}
