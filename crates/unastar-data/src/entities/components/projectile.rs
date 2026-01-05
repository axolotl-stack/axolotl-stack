use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:projectile`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Projectile {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
