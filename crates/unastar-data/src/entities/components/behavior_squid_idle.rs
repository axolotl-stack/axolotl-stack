use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSquidIdlePriority {}
impl Default for BehaviorSquidIdlePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.squid_idle`. Allows the squid to swim in place idly. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidIdle {
    ///priority
    pub priority: Option<BehaviorSquidIdlePriority>,
}
impl Default for BehaviorSquidIdle {
    fn default() -> Self {
        Self { priority: None }
    }
}
