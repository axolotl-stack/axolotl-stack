use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorOfferFlowerPriority {}
impl Default for BehaviorOfferFlowerPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.offer_flower`. Allows the mob to offer the player a flower like the Iron Golem does.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOfferFlower {
    ///Percent chance that the mob will start this goal from 0.0 to 1.0 (where 1.0 = 100%).
    pub chance_to_start: Option<f32>,
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub max_head_rotation_y: Option<f32>,
    ///The max amount of time (in seconds) that the mob will offer the flower for before exiting the Goal.
    pub max_offer_flower_duration: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub max_rotation_x: Option<f32>,
    ///priority
    pub priority: Option<BehaviorOfferFlowerPriority>,
    ///The dimensions of the AABB used to search for a potential mob to offer flower to.
    pub search_area: Option<Vec<f32>>,
}
impl Default for BehaviorOfferFlower {
    fn default() -> Self {
        Self {
            chance_to_start: Some(0f32),
            filters: None,
            max_head_rotation_y: Some(30f32),
            max_offer_flower_duration: Some(20f32),
            max_rotation_x: Some(30f32),
            priority: None,
            search_area: Some(vec![6f32, 2f32, 6f32]),
        }
    }
}
