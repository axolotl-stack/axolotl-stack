use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.raid_garden`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRaidGarden {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
