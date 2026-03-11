use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_hurt_by_player`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnHurtByPlayer {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
