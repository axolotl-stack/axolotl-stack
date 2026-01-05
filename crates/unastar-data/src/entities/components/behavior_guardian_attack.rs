use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.guardian_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorGuardianAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
