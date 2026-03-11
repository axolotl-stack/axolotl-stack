use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.stay_while_sitting`. Allows the mob to stay put while it is in a sitting state instead of doing something else.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStayWhileSitting {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorStayWhileSitting {
    fn default() -> Self {
        Self { priority: None }
    }
}
