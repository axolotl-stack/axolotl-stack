use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSquidDivePriority {}
impl Default for BehaviorSquidDivePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.squid_dive`. Allows an entity to dive underwater.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidDive {
    ///priority
    pub priority: Option<BehaviorSquidDivePriority>,
}
impl Default for BehaviorSquidDive {
    fn default() -> Self {
        Self { priority: None }
    }
}
