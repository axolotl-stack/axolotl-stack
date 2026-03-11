use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTimerFlag1ControlFlags {}
impl Default for BehaviorTimerFlag1ControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTimerFlag1Priority {}
impl Default for BehaviorTimerFlag1Priority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.timer_flag_1`. Fires an event when this behavior starts, then waits for a duration before stopping. When stopping due to that timeout or due to being interrupted by another behavior, fires another event. query.timer_flag_<n> will return 1.0 on both the client and server when this behavior is running, and 0.0 otherwise.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTimerFlag1 {
    ///control_flags
    pub control_flags: Option<BehaviorTimerFlag1ControlFlags>,
    ///Goal cooldown range in seconds.
    pub cooldown_range: Option<Vec<f32>>,
    ///Goal duration range in seconds.
    pub duration_range: Option<Vec<f32>>,
    ///Event to run when the goal ends.
    pub on_end: Option<crate::types::BedrockValue>,
    ///Event to run when the goal starts.
    pub on_start: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorTimerFlag1Priority>,
}
impl Default for BehaviorTimerFlag1 {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorTimerFlag1ControlFlags {}),
            cooldown_range: Some(vec![0f32]),
            duration_range: Some(vec![0f32]),
            on_end: Some(crate::types::BedrockValue::Object(
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
            priority: Some(BehaviorTimerFlag1Priority {}),
        }
    }
}
