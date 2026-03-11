use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct TargetNearbySensorOnInsideRange {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for TargetNearbySensorOnInsideRange {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TargetNearbySensorOnOutsideRange {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for TargetNearbySensorOnOutsideRange {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TargetNearbySensorOnVisionLostInsideRange {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for TargetNearbySensorOnVisionLostInsideRange {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:target_nearby_sensor`. Defines the entity's range within which it can see or sense other entities to target them.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct TargetNearbySensor {
    ///Maximum distance in blocks that another entity will be considered in the `inside` range.
    pub inside_range: Option<f32>,
    ///Whether the other entity needs to be visible to trigger `inside` events.
    pub must_see: Option<bool>,
    ///Event to call when an entity gets in the inside range. Can specify `event` for the name of the event and `target` for the target of the event
    pub on_inside_range: Option<TargetNearbySensorOnInsideRange>,
    ///Event to call when an entity gets in the outside range. Can specify `event` for the name of the event and `target` for the target of the event
    pub on_outside_range: Option<TargetNearbySensorOnOutsideRange>,
    ///Event to call when an entity exits visual range. Can specify `event` for the name of the event and `target` for the target of the event
    pub on_vision_lost_inside_range: Option<TargetNearbySensorOnVisionLostInsideRange>,
    ///Maximum distance in blocks that another entity will be considered in the `outside` range.
    pub outside_range: Option<f32>,
}
impl Default for TargetNearbySensor {
    fn default() -> Self {
        Self {
            inside_range: Some(1f32),
            must_see: Some(false),
            on_inside_range: None,
            on_outside_range: None,
            on_vision_lost_inside_range: None,
            outside_range: Some(5f32),
        }
    }
}
