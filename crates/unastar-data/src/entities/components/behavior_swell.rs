use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.swell`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSwell {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
