use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:experience_reward`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct ExperienceReward {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
