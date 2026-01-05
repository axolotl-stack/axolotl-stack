use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:game_event_movement_tracking`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct GameEventMovementTracking {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
