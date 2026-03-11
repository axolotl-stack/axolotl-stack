use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSummonEntityPriority {}
impl Default for BehaviorSummonEntityPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSummonEntitySummonChoicesSequence {
    ///Amount of time in seconds to wait before this step starts.
    pub base_delay: Option<f32>,
    ///Amount of time in seconds to wait before this step starts.
    pub delay: Option<f32>,
    ///Amount of time in seconds before each entity is summoned in this step.
    pub delay_per_summon: Option<f32>,
    ///Amount of time in seconds that the spawned entity will be alive for. A value of -1.0 means it will remain alive for as long as it can
    pub entity_lifespan: Option<f32>,
    ///The entity type of the entities we will spawn in this step.
    pub entity_type: Option<String>,
    ///Number of entities that will be spawned in this step.
    pub num_entities_spawned: Option<i32>,
    ///The base shape of this step. Valid values are circle and line
    pub shape: Option<String>,
    ///The base size of the entity.
    pub size: Option<f32>,
    ///The sound event to play for this step.
    pub sound_event: Option<String>,
    ///Maximum number of summoned entities at any given time.
    pub summon_cap: Option<i32>,
    ///Maximum radius where the summon entities can spawn.
    pub summon_cap_radius: Option<f32>,
    ///Event to invoke on each summoned entity on spawn.
    pub summon_event: Option<crate::types::BedrockValue>,
    ///The target of the spell. This is where the spell will start (line will start here, circle will be centered here)
    pub target: Option<String>,
}
impl Default for BehaviorSummonEntitySummonChoicesSequence {
    fn default() -> Self {
        Self {
            base_delay: None,
            delay: None,
            delay_per_summon: None,
            entity_lifespan: None,
            entity_type: None,
            num_entities_spawned: None,
            shape: None,
            size: None,
            sound_event: None,
            summon_cap: None,
            summon_cap_radius: None,
            summon_event: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSummonEntitySummonChoices {
    ///Time in seconds the spell casting will take.
    pub cast_duration: Option<f32>,
    ///Time in seconds the mob has to wait before using the spell again.
    pub cooldown_time: Option<f32>,
    ///If true, the mob will do the casting animations and render spell particles.
    pub do_casting: Option<bool>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///Upper bound of the activation distance in blocks for this spell.
    pub max_activation_range: Option<f32>,
    ///Lower bound of the activation distance in blocks for this spell.
    pub min_activation_range: Option<f32>,
    ///The color of the particles for this spell.
    pub particle_color: Option<crate::types::MolangOr<i32>>,
    ///List of steps for the spell.
    pub sequence: Option<Vec<BehaviorSummonEntitySummonChoicesSequence>>,
    ///The sound event to play when using this spell.
    pub start_sound_event: Option<String>,
    ///The weight of this spell. Controls how likely the mob is to choose this spell when casting one
    pub weight: Option<f32>,
}
impl Default for BehaviorSummonEntitySummonChoices {
    fn default() -> Self {
        Self {
            cast_duration: None,
            cooldown_time: None,
            do_casting: None,
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
/// Bedrock component `minecraft:behavior.summon_entity`. Allows the mob to attack the player by summoning other entities.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSummonEntity {
    ///priority
    pub priority: Option<BehaviorSummonEntityPriority>,
    ///List of spells for the mob to use to summon entities.
    pub summon_choices: Option<Vec<BehaviorSummonEntitySummonChoices>>,
}
impl Default for BehaviorSummonEntity {
    fn default() -> Self {
        Self {
            priority: None,
            summon_choices: None,
        }
    }
}
