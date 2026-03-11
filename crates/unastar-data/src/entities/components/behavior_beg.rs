use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorBegPriority {}
impl Default for BehaviorBegPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.beg`. Allows this mob to look at and follow the player that holds food they like.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorBeg {
    ///List of items that this mob likes.
    pub items: Option<Vec<crate::types::BedrockValue>>,
    ///Distance in blocks the mob will beg from.
    pub look_distance: Option<f32>,
    ///The range of time in seconds this mob will stare at the player holding a food they like, begging for it.
    pub look_time: Option<crate::types::RangeOrVal<f32>>,
    ///priority
    pub priority: Option<BehaviorBegPriority>,
}
impl Default for BehaviorBeg {
    fn default() -> Self {
        Self {
            items: None,
            look_distance: Some(8f32),
            look_time: None,
            priority: None,
        }
    }
}
