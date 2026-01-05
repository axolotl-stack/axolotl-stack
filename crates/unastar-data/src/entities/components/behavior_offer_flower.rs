use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.offer_flower`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorOfferFlower {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
