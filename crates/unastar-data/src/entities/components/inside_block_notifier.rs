use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:inside_block_notifier`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct InsideBlockNotifier {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
