use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.amphibious`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementAmphibious {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
