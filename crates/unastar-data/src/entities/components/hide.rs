use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:hide`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Hide {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
