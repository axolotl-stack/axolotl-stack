use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSquidFleePriority {}
impl Default for BehaviorSquidFleePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.squid_flee`. Allows the squid to swim away. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidFlee {
    ///priority
    pub priority: Option<BehaviorSquidFleePriority>,
}
impl Default for BehaviorSquidFlee {
    fn default() -> Self {
        Self { priority: None }
    }
}
