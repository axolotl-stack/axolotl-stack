use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalBandwidthOptimizationConditionalValues {
    ///Conditions that must be met for these optimization values to be used.
    pub conditional_values: Option<Vec<crate::types::BedrockValue>>,
    ///In relation to the optimization value, determines the maximum ticks spatial update packets can be not sent.
    pub max_dropped_ticks: Option<i32>,
    ///The maximum distance considered during bandwidth optimizations. Any value below the Maximum is interpolated to find optimization, and any value greater than or equal to this Maximum results in Maximum optimization.
    pub max_optimized_distance: Option<f32>,
    ///When set to true, smaller motion packets will be sent during drop packet intervals, resulting in the same amount of packets being sent as without optimizations but with much less data being sent. This should be used when actors are travelling very quickly or teleporting to prevent visual oddities.
    pub use_motion_prediction_hints: Option<bool>,
}
impl Default for ConditionalBandwidthOptimizationConditionalValues {
    fn default() -> Self {
        Self {
            conditional_values: None,
            max_dropped_ticks: None,
            max_optimized_distance: None,
            use_motion_prediction_hints: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalBandwidthOptimizationDefaultValues {
    ///In relation to the optimization value, determines the maximum ticks spatial update packets can be not sent.
    pub max_dropped_ticks: Option<i32>,
    ///The maximum distance considered during bandwidth optimizations. Any value below the Maximum is interpolated to find optimization, and any value greater than or equal to this Maximum results in Maximum optimization.
    pub max_optimized_distance: Option<f32>,
    ///When set to true, smaller motion packets will be sent during drop packet intervals, resulting in the same amount of packets being sent as without optimizations but with much less data being sent. This should be used when actors are travelling very quickly or teleporting to prevent visual oddities.
    pub use_motion_prediction_hints: Option<bool>,
}
impl Default for ConditionalBandwidthOptimizationDefaultValues {
    fn default() -> Self {
        Self {
            max_dropped_ticks: None,
            max_optimized_distance: None,
            use_motion_prediction_hints: None,
        }
    }
}
/// Bedrock component `minecraft:conditional_bandwidth_optimization`. Defines the Conditional Spatial Update Bandwidth Optimizations of this entity.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ConditionalBandwidthOptimization {
    ///The object containing the conditional bandwidth optimization values.
    pub conditional_values: Option<Vec<ConditionalBandwidthOptimizationConditionalValues>>,
    ///The object containing the default bandwidth optimization values.
    pub default_values: Option<ConditionalBandwidthOptimizationDefaultValues>,
}
impl Default for ConditionalBandwidthOptimization {
    fn default() -> Self {
        Self {
            conditional_values: None,
            default_values: None,
        }
    }
}
