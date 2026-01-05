use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.nearest_attackable_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorNearestAttackableTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
