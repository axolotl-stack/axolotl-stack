use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:combat_regeneration`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct CombatRegeneration {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
