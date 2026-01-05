use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.squid_out_of_water`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSquidOutOfWater {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
