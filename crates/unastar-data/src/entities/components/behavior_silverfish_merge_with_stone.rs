use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.silverfish_merge_with_stone`. Allows the mob to go into stone blocks like Silverfish do. Currently it can only be used by Silverfish.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSilverfishMergeWithStone {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorSilverfishMergeWithStone {
    fn default() -> Self {
        Self { priority: None }
    }
}
