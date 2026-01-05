use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.wither_random_attack_pos_goal`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorWitherRandomAttackPosGoal {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
