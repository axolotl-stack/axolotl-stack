use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:renders_when_invisible`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct RendersWhenInvisible {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
