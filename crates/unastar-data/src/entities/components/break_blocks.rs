use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:break_blocks`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BreakBlocks {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
