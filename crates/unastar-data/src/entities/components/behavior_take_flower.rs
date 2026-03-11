use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTakeFlowerPriority {}
impl Default for BehaviorTakeFlowerPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTakeFlowerSpeedMultiplier {}
impl Default for BehaviorTakeFlowerSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.take_flower`. Can only be used by Villagers. Allows the mob to accept flowers from Iron Golems.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTakeFlower {
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub max_head_rotation_y: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub max_rotation_x: Option<f32>,
    ///The maximum amount of time (in seconds) for the mob to randomly wait for before taking the flower.
    pub max_wait_time: Option<f32>,
    ///Minimum distance (in blocks) for the entity to be considered having reached its target.
    pub min_distance_to_target: Option<f32>,
    ///The minimum amount of time (in seconds) for the mob to randomly wait for before taking the flower.
    pub min_wait_time: Option<f32>,
    ///Event triggered when the entity takes a flower from another entity.
    pub on_take_flower: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorTakeFlowerPriority>,
    ///The dimensions of the AABB used to search for a potential mob to take a flower from.
    pub search_area: Option<Vec<f32>>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorTakeFlowerSpeedMultiplier>,
}
impl Default for BehaviorTakeFlower {
    fn default() -> Self {
        Self {
            filters: None,
            max_head_rotation_y: Some(30f32),
            max_rotation_x: Some(30f32),
            max_wait_time: Some(20f32),
            min_distance_to_target: Some(2f32),
            min_wait_time: Some(4f32),
            on_take_flower: None,
            priority: None,
            search_area: Some(vec![6f32, 2f32, 6f32]),
            speed_multiplier: Some(BehaviorTakeFlowerSpeedMultiplier {}),
        }
    }
}
