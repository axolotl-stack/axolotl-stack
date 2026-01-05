use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:genetics`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Genetics {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
