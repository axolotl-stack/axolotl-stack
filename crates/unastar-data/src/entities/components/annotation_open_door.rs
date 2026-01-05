use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:annotation.open_door`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct AnnotationOpenDoor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
