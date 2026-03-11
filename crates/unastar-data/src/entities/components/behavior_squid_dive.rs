use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.squid_dive`. Allows an entity to dive underwater.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidDive {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorSquidDive {
    fn default() -> Self {
        Self { priority: None }
    }
}
