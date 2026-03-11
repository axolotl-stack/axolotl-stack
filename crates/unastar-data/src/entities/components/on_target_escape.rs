use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_target_escape`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnTargetEscape {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
