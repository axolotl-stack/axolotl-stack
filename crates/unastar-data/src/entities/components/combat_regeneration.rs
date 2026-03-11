use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:combat_regeneration`. Gives Regeneration I and removes Mining Fatigue from the mob that kills the Actor`s attack target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct CombatRegeneration {
    ///Determines if the mob will grant mobs of the same type combat buffs if they kill the target.
    pub apply_to_family: Option<bool>,
    ///Determines if the mob will grant itself the combat buffs if it kills the target.
    pub apply_to_self: Option<bool>,
    ///The duration in seconds of Regeneration I added to the mob.
    pub regeneration_duration: Option<crate::types::MolangOr<i32>>,
}
impl Default for CombatRegeneration {
    fn default() -> Self {
        Self {
            apply_to_family: Some(false),
            apply_to_self: Some(false),
            regeneration_duration: Some(crate::types::MolangOr::Value(5i32)),
        }
    }
}
