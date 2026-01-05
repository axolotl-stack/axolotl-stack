use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:shareables`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Shareables {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
