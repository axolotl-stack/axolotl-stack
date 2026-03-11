use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:game_event_movement_tracking`. Allows an entity to emit `entityMove`, `swim` and `flap` game events, depending on the block the entity is moving through. It is added by default to every mob. Add it again to override its behavior.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct GameEventMovementTracking {
    /// emit_flap
    pub emit_flap: Option<bool>,
    /// emit_move
    pub emit_move: Option<bool>,
    /// emit_swim
    pub emit_swim: Option<bool>,
}
impl Default for GameEventMovementTracking {
    fn default() -> Self {
        Self {
            emit_flap: Some(false),
            emit_move: Some(true),
            emit_swim: Some(true),
        }
    }
}
