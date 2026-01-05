use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:explode`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Explode {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
