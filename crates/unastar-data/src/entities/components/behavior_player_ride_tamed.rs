use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.player_ride_tamed`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorPlayerRideTamed {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
