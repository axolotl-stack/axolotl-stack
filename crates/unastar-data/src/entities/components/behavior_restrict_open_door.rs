use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.restrict_open_door`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRestrictOpenDoor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
