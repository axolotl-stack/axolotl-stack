use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.silverfish_merge_with_stone`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSilverfishMergeWithStone {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
