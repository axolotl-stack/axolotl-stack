use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:dimension_bound`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct DimensionBound {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
