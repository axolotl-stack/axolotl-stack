use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.open_door`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorOpenDoor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
