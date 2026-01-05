use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.melee_box_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMeleeBoxAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
