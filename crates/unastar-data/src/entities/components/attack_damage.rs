use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:attack_damage`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct AttackDamage {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
