use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorCroakPriority {}
impl Default for BehaviorCroakPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.croak`. [EXPERIMENTAL BEHAVIOR] Allows the entity to croak at a random time interval with configurable conditions.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorCroak {
    ///Random range in seconds after which the croaking stops. Can also be a constant.
    pub duration: Option<Vec<f32>>,
    ///Conditions for the behavior to start and keep running. The interval between runs only starts after passing the filters.
    pub filters: Option<crate::types::BedrockValue>,
    ///Random range in seconds between runs of this behavior. Can also be a constant.
    pub interval: Option<Vec<i32>>,
    ///priority
    pub priority: Option<BehaviorCroakPriority>,
}
impl Default for BehaviorCroak {
    fn default() -> Self {
        Self {
            duration: None,
            filters: None,
            interval: None,
            priority: None,
        }
    }
}
