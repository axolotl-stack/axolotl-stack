use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorBarterPriority {}
impl Default for BehaviorBarterPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.barter`. Enables the mob to barter for items that have been configured as barter currency. Must be used in combination with the barter component
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorBarter {
    ///priority
    pub priority: Option<BehaviorBarterPriority>,
}
impl Default for BehaviorBarter {
    fn default() -> Self {
        Self { priority: None }
    }
}
