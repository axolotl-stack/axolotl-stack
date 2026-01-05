use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.beg`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorBeg {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
