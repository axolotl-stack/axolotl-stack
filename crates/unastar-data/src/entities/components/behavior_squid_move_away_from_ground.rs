use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSquidMoveAwayFromGroundPriority {}
impl Default for BehaviorSquidMoveAwayFromGroundPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.squid_move_away_from_ground`. Allows the squid to move away from ground blocks and back to water. Can only be used by the Squid.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSquidMoveAwayFromGround {
    ///priority
    pub priority: Option<BehaviorSquidMoveAwayFromGroundPriority>,
}
impl Default for BehaviorSquidMoveAwayFromGround {
    fn default() -> Self {
        Self { priority: None }
    }
}
