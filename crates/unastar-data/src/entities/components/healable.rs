use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:healable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Healable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
