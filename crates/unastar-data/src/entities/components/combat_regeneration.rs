use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:combat_regeneration`. Gives Regeneration I and removes Mining Fatigue from the mob that kills the Actor`s attack target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct CombatRegeneration {
    /// apply_to_family
    pub apply_to_family: Option<bool>,
    /// apply_to_self
    pub apply_to_self: Option<bool>,
    /// regeneration_duration
    pub regeneration_duration: Option<String>,
}
impl Default for CombatRegeneration {
    fn default() -> Self {
        Self {
            apply_to_family: Some(false),
            apply_to_self: Some(false),
            regeneration_duration: Some("5".to_string()),
        }
    }
}
