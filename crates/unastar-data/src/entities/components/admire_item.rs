use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:admire_item`. Causes the mob to ignore attackable targets for a given duration.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct AdmireItem {
    ///Duration, in seconds, for which mob won't admire items if it was hurt.
    pub cooldown_after_being_attacked: Option<i32>,
    ///Duration, in seconds, that the mob is pacified.
    pub duration: Option<i32>,
}
impl Default for AdmireItem {
    fn default() -> Self {
        Self {
            cooldown_after_being_attacked: Some(0i32),
            duration: Some(10i32),
        }
    }
}
