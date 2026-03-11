use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:experience_reward`. Defines the amount of experience rewarded when the entity dies or is successfully bred.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ExperienceReward {
    ///A molang expression defining the amount of experience rewarded when this entity is successfully bred. An array of expressions adds each expression's result together for a final total.
    pub on_bred: Option<crate::types::MolangOr<f32>>,
    ///A molang expression defining the amount of experience rewarded when this entity dies. An array of expressions adds each expression's result together for a final total.
    pub on_death: Option<crate::types::MolangOr<f32>>,
}
impl Default for ExperienceReward {
    fn default() -> Self {
        Self {
            on_bred: Some(crate::types::MolangOr::Value(0f32)),
            on_death: Some(crate::types::MolangOr::Value(0f32)),
        }
    }
}
