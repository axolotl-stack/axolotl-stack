use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.swim_with_entity`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSwimWithEntity {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
