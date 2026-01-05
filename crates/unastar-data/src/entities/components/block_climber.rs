use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:block_climber`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BlockClimber {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
