use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.offer_flower`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOfferFlower {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
