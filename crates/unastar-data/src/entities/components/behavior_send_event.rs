use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSendEventEventChoicesSequence {
    ///Amount of time in seconds before starting this step.
    pub base_delay: Option<f32>,
    ///The event to send to the entity.
    pub event: Option<String>,
    ///The sound event to play when this step happens.
    pub sound_event: Option<String>,
}
impl Default for BehaviorSendEventEventChoicesSequence {
    fn default() -> Self {
        Self {
            base_delay: None,
            event: None,
            sound_event: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSendEventEventChoices {
    ///Time in seconds the spell casting will take.
    pub cast_duration: Option<f32>,
    ///Time in seconds before the mob can use this spell again.
    pub cooldown_time: Option<f32>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The maxmimum distance in blocks the target must be for this spell to be cast.
    pub max_activation_range: Option<f32>,
    ///The minimum distance in blocks the target must be for this spell to be cast.
    pub min_activation_range: Option<f32>,
    ///The color of the particles for this spell.
    pub particle_color: Option<String>,
    ///sequence
    pub sequence: Option<Vec<BehaviorSendEventEventChoicesSequence>>,
    ///The sound event to play when using this spell.
    pub start_sound_event: Option<String>,
    ///The weight of this spell. Controls how likely this spell will be picked
    pub weight: Option<i32>,
}
impl Default for BehaviorSendEventEventChoices {
    fn default() -> Self {
        Self {
            cast_duration: None,
            cooldown_time: None,
            filters: None,
            max_activation_range: None,
            min_activation_range: None,
            particle_color: None,
            sequence: None,
            start_sound_event: None,
            weight: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSendEventPriority {}
impl Default for BehaviorSendEventPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSendEventSequence {
    ///Amount of time in seconds before starting this step.
    pub base_delay: Option<f32>,
    ///The event to send to the entity.
    pub event: Option<String>,
    ///The sound event to play when this step happens.
    pub sound_event: Option<String>,
}
impl Default for BehaviorSendEventSequence {
    fn default() -> Self {
        Self {
            base_delay: None,
            event: None,
            sound_event: None,
        }
    }
}
/// Bedrock component `minecraft:behavior.send_event`. Allows the mob to send an event to another mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSendEvent {
    ///Time in seconds for the entire event sending process.
    pub cast_duration: Option<f32>,
    ///List of spells for the mob to use.
    pub event_choices: Option<Vec<BehaviorSendEventEventChoices>>,
    ///If true, the mob will face the entity it sends an event to.
    pub look_at_target: Option<bool>,
    ///priority
    pub priority: Option<BehaviorSendEventPriority>,
    ///List of steps for the spell.
    pub sequence: Option<Vec<BehaviorSendEventSequence>>,
}
impl Default for BehaviorSendEvent {
    fn default() -> Self {
        Self {
            cast_duration: None,
            event_choices: None,
            look_at_target: Some(true),
            priority: None,
            sequence: None,
        }
    }
}
