use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct RavagerBlockedReactionChoices {
    ///An event that runs when this reaction is picked.
    pub value: crate::types::BedrockValue,
    ///The chance of this reaction being picked.
    pub weight: Option<i32>,
}
impl Default for RavagerBlockedReactionChoices {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
            weight: None,
        }
    }
}
/// Bedrock component `minecraft:ravager_blocked`. Defines the ravager's response to their melee attack being blocked.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct RavagerBlocked {
    ///The strength with which blocking entities should be knocked back.
    pub knockback_strength: Option<f32>,
    ///A list of weighted responses to the melee attack being blocked.
    pub reaction_choices: Option<Vec<RavagerBlockedReactionChoices>>,
}
impl Default for RavagerBlocked {
    fn default() -> Self {
        Self {
            knockback_strength: Some(3f32),
            reaction_choices: Some(vec![RavagerBlockedReactionChoices {
                value: crate::types::BedrockValue::Null,
                weight: None,
            }]),
        }
    }
}
