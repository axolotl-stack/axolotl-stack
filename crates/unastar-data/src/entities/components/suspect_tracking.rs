use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:suspect_tracking`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct SuspectTracking {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
