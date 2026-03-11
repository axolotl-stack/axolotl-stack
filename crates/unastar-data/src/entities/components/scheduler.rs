use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulerScheduledEvents {
    ///event
    pub event: Option<crate::types::BedrockValue>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
}
impl Default for SchedulerScheduledEvents {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
        }
    }
}
/// Bedrock component `minecraft:scheduler`. fires off scheduled mob events at time of day events.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Scheduler {
    ///The maximum the scheduler will be delayed.
    pub max_delay_secs: Option<f32>,
    ///The minimum the scheduler will be delayed.
    pub min_delay_secs: Option<f32>,
    ///The list of triggers that fire when the conditions match the given filter criteria. If any filter criteria overlap the first defined event will be picked.
    pub scheduled_events: Option<Vec<SchedulerScheduledEvents>>,
}
impl Default for Scheduler {
    fn default() -> Self {
        Self {
            max_delay_secs: None,
            min_delay_secs: None,
            scheduled_events: None,
        }
    }
}
