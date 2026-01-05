use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_indoors`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveIndoors {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
