use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.charge_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorChargeAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
