use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_hurt`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct OnHurt {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
