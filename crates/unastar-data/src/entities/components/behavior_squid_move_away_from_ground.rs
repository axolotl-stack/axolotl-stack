use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.squid_move_away_from_ground`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSquidMoveAwayFromGround {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
