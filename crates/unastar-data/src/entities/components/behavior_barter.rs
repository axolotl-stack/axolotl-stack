use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.barter`. Enables the mob to barter for items that have been configured as barter currency. Must be used in combination with the barter component
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorBarter {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorBarter {
    fn default() -> Self {
        Self { priority: None }
    }
}
