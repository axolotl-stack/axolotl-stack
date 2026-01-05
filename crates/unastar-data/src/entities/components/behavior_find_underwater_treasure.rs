use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.find_underwater_treasure`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorFindUnderwaterTreasure {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
