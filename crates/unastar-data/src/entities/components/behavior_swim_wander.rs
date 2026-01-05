use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.swim_wander`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSwimWander {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
