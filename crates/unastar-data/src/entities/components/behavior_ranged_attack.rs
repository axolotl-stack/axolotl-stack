use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.ranged_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRangedAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
