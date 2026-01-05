use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_stroll`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomStroll {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
