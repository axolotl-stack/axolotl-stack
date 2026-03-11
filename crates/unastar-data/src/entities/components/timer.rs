use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct TimerRandomTimeChoices {
    ///The value in seconds that would be used if this section was picked.
    pub value: i32,
    ///The weight on how likely this section is to trigger.
    pub weight: Option<i32>,
}
impl Default for TimerRandomTimeChoices {
    fn default() -> Self {
        Self {
            value: 0i32,
            weight: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TimerTimeDownEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for TimerTimeDownEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:timer`. Adds a timer after which an event will fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Timer {
    ///If true, the timer will restart every time after it fires.
    pub looping: Option<bool>,
    ///If true, the amount of time on the timer will be random between the Minimum and Maximum values specified in time.
    pub random_interval: Option<bool>,
    ///This is a list of objects, representing one value in seconds that can be picked before firing the event and an optional weight. Incompatible with time.
    pub random_time_choices: Option<Vec<TimerRandomTimeChoices>>,
    ///Amount of time in seconds for the timer. Can be specified as a number or a pair of numbers (Minimum and max). Incompatible with random_time_choices.
    pub time: Option<crate::types::RangeOrVal<f32>>,
    ///Event to fire when the time on the timer runs out.
    pub time_down_event: Option<TimerTimeDownEvent>,
}
impl Default for Timer {
    fn default() -> Self {
        Self {
            looping: Some(true),
            random_interval: Some(true),
            random_time_choices: Some(vec![TimerRandomTimeChoices {
                value: 0i32,
                weight: None,
            }]),
            time: None,
            time_down_event: None,
        }
    }
}
