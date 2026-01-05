use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:shooter`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Shooter {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
