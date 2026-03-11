use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.squid_idle`. Allows the squid to swim in place idly. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidIdle {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorSquidIdle {
    fn default() -> Self {
        Self { priority: None }
    }
}
