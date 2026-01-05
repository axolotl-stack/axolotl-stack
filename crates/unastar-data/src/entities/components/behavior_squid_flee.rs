use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.squid_flee`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSquidFlee {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
