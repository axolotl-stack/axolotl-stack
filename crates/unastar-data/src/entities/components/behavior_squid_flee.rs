use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.squid_flee`. Allows the squid to swim away. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidFlee {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorSquidFlee {
    fn default() -> Self {
        Self { priority: None }
    }
}
