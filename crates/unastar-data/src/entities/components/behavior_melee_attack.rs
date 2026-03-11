use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.melee_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMeleeAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
