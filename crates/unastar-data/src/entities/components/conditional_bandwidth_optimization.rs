use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:conditional_bandwidth_optimization`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct ConditionalBandwidthOptimization {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
