use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDrinkMilkControlFlags {}
impl Default for BehaviorDrinkMilkControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDrinkMilkPriority {}
impl Default for BehaviorDrinkMilkPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.drink_milk`. Allows the mob to drink milk based on specified environment conditions.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorDrinkMilk {
    ///control_flags
    pub control_flags: Option<BehaviorDrinkMilkControlFlags>,
    ///Time (in seconds) that the goal is on cooldown before it can be used again.
    pub cooldown_seconds: Option<f32>,
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorDrinkMilkPriority>,
}
impl Default for BehaviorDrinkMilk {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorDrinkMilkControlFlags {}),
            cooldown_seconds: Some(5f32),
            filters: Some(crate::types::BedrockValue::Object(
                std::collections::HashMap::from([
                    ("AND".to_string(), crate::types::BedrockValue::Null),
                    ("NOT".to_string(), crate::types::BedrockValue::Null),
                    ("OR".to_string(), crate::types::BedrockValue::Null),
                    ("all".to_string(), crate::types::BedrockValue::Null),
                    ("all_of".to_string(), crate::types::BedrockValue::Null),
                    ("any".to_string(), crate::types::BedrockValue::Null),
                    ("any_of".to_string(), crate::types::BedrockValue::Null),
                    ("none_of".to_string(), crate::types::BedrockValue::Null),
                ]),
            )),
            priority: Some(BehaviorDrinkMilkPriority {}),
        }
    }
}
