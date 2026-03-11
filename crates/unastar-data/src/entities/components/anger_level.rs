use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct AngerLevelOnIncreaseSounds {
    ///The event that will trigger the sound
    pub condition: String,
    ///The sound to play
    pub sound: String,
}
impl Default for AngerLevelOnIncreaseSounds {
    fn default() -> Self {
        Self {
            condition: "".to_string(),
            sound: "".to_string(),
        }
    }
}
/// Bedrock component `minecraft:anger_level`. Allows this entity to track anger towards a set of nuisances
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct AngerLevel {
    ///Anger level will decay over time. Defines how often anger towards all nuisances will be decreased by one
    pub anger_decrement_interval: Option<f32>,
    ///Anger boost applied to angry threshold when mob gets angry.
    pub angry_boost: Option<i32>,
    ///Threshold that define when the mob is considered angry at a nuisance.
    pub angry_threshold: Option<i32>,
    ///The default amount of annoyingness for any given nuisance. Specifies how much to raise anger level on each provocation
    pub default_annoyingness: Option<f32>,
    ///The default amount of annoyingness for projectile nuisance. Specifies how much to raise anger level on each provocation
    pub default_projectile_annoyingness: Option<f32>,
    ///The maximum anger level that can be reached. Applies to any nuisance
    pub max_anger: Option<i32>,
    ///Filter that is applied to determine if a mob can be a nuisance.
    pub nuisance_filter: Option<crate::types::BedrockValue>,
    ///On Increase Sounds
    pub on_increase_sounds: Option<Vec<AngerLevelOnIncreaseSounds>>,
    ///Defines if the mob should remove target if it falls below 'angry' threshold.
    pub remove_targets_below_angry_threshold: Option<bool>,
}
impl Default for AngerLevel {
    fn default() -> Self {
        Self {
            anger_decrement_interval: Some(1f32),
            angry_boost: Some(20i32),
            angry_threshold: Some(80i32),
            default_annoyingness: Some(0f32),
            default_projectile_annoyingness: None,
            max_anger: Some(100i32),
            nuisance_filter: None,
            on_increase_sounds: None,
            remove_targets_below_angry_threshold: Some(true),
        }
    }
}
