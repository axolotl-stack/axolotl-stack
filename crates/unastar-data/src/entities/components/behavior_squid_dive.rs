use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.squid_dive`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSquidDive {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
