use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:burns_in_daylight`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BurnsInDaylight {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
