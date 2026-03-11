use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:experience_reward`. Defines the amount of experience rewarded when the entity dies or is successfully bred.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ExperienceReward {
    /// on_bred
    pub on_bred: Option<String>,
    /// on_death
    pub on_death: Option<String>,
}
impl Default for ExperienceReward {
    fn default() -> Self {
        Self {
            on_bred: None,
            on_death: None,
        }
    }
}
