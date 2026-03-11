use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.nearest_prioritized_attackable_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorNearestPrioritizedAttackableTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
