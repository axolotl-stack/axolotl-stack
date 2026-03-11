use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:game_event_movement_tracking`. Allows an entity to emit `entityMove`, `swim` and `flap` game events, depending on the block the entity is moving through. It is added by default to every mob. Add it again to override its behavior.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct GameEventMovementTracking {
    ///If true, the `flap` game event will be emitted when the entity moves through air.
    pub emit_flap: Option<bool>,
    ///If true, the `entityMove` game event will be emitted when the entity moves on ground or through a solid.
    pub emit_move: Option<bool>,
    ///If true, the `swim` game event will be emitted when the entity moves through a liquid.
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
