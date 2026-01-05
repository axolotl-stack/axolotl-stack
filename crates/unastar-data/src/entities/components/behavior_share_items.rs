use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.share_items`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorShareItems {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
