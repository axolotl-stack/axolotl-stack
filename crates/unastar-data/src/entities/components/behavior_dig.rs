use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDigControlFlags {}
impl Default for BehaviorDigControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDigPriority {}
impl Default for BehaviorDigPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.dig`. [EXPERIMENTAL BEHAVIOR] Activates the `DIGGING` actor flag during the specified duration. Currently only Warden can use the Dig goal
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorDig {
    ///If true, this behavior can run when this entity is named. Otherwise not.
    pub allow_dig_when_named: Option<bool>,
    ///control_flags
    pub control_flags: Option<BehaviorDigControlFlags>,
    ///Indicates that the actor should start digging when it sees daylight.
    pub digs_in_daylight: Option<bool>,
    ///Goal duration in seconds.
    pub duration: Option<f32>,
    ///The minimum idle time in seconds between the last detected disturbance to the start of digging.
    pub idle_time: Option<f32>,
    ///The event to run when the goal start
    pub on_start: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorDigPriority>,
    ///If true, finding new suspicious locations count as disturbances that may delay the start of this goal.
    pub suspicion_is_disturbance: Option<bool>,
    ///If true, vibrations count as disturbances that may delay the start of this goal.
    pub vibration_is_disturbance: Option<bool>,
}
impl Default for BehaviorDig {
    fn default() -> Self {
        Self {
            allow_dig_when_named: Some(false),
            control_flags: Some(BehaviorDigControlFlags {}),
            digs_in_daylight: Some(false),
            duration: Some(0f32),
            idle_time: Some(0f32),
            on_start: Some(crate::types::BedrockValue::Object(
                std::collections::HashMap::from([
                    (
                        "event".to_string(),
                        crate::types::BedrockValue::String("".to_string()),
                    ),
                    (
                        "filters".to_string(),
                        crate::types::BedrockValue::Object(std::collections::HashMap::from([
                            ("AND".to_string(), crate::types::BedrockValue::Null),
                            ("NOT".to_string(), crate::types::BedrockValue::Null),
                            ("OR".to_string(), crate::types::BedrockValue::Null),
                            ("all".to_string(), crate::types::BedrockValue::Null),
                            ("all_of".to_string(), crate::types::BedrockValue::Null),
                            ("any".to_string(), crate::types::BedrockValue::Null),
                            ("any_of".to_string(), crate::types::BedrockValue::Null),
                            ("none_of".to_string(), crate::types::BedrockValue::Null),
                        ])),
                    ),
                    (
                        "target".to_string(),
                        crate::types::BedrockValue::String("self".to_string()),
                    ),
                ]),
            )),
            priority: Some(BehaviorDigPriority {}),
            suspicion_is_disturbance: Some(false),
            vibration_is_disturbance: Some(false),
        }
    }
}
