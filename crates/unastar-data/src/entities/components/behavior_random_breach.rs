use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_breach`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomBreach {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
