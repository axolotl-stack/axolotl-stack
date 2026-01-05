use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:jump.static`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct JumpStatic {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
