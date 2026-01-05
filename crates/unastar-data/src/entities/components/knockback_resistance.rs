use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:knockback_resistance`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct KnockbackResistance {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
