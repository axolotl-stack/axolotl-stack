use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.circle_around_anchor`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorCircleAroundAnchor {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
