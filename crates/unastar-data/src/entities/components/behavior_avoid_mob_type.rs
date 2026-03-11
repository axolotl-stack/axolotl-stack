use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAvoidMobTypeControlFlags {}
impl Default for BehaviorAvoidMobTypeControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAvoidMobTypeEntityTypes {
    ///UNDOCUMENTED.
    pub check_if_outnumbered: Option<bool>,
    ///The amount of time in seconds that the mob has to wait before selecting a target of the same type again
    pub cooldown: Option<f32>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///Maximum distance this mob can be away to be a valid choice.
    pub max_dist: Option<f32>,
    ///UNDOCUMENTED.
    pub max_flee: Option<f32>,
    ///UNDOCUMENTED.
    pub max_height: Option<f32>,
    ///If true, the mob has to be visible to be a valid choice.
    pub must_see: Option<bool>,
    ///Determines the amount of time in seconds that this mob will look for a target before forgetting about it and looking for a new one when the target isn't visible any more.
    pub must_see_forget_duration: Option<f32>,
    ///UNDOCUMENTED.
    pub priority: Option<f32>,
    ///If true, the mob will stop being targeted if it stops meeting any conditions.
    pub reevaluate_description: Option<bool>,
    ///Multiplier for the running speed. A value of 1.0 means the speed is unchanged
    pub sprint_speed_multiplier: Option<f32>,
    ///Multiplier for the walking speed. A value of 1.0 means the speed is unchanged
    pub walk_speed_multiplier: Option<f32>,
    ///UNDOCUMENTED.
    pub within_default: Option<f32>,
}
impl Default for BehaviorAvoidMobTypeEntityTypes {
    fn default() -> Self {
        Self {
            check_if_outnumbered: None,
            cooldown: None,
            filters: None,
            max_dist: None,
            max_flee: None,
            max_height: None,
            must_see: None,
            must_see_forget_duration: None,
            priority: None,
            reevaluate_description: None,
            sprint_speed_multiplier: None,
            walk_speed_multiplier: None,
            within_default: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAvoidMobTypePriority {}
impl Default for BehaviorAvoidMobTypePriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.avoid_mob_type`. Allows the entity to run away from other entities that meet the criteria specified.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorAvoidMobType {
    ///The sound event to play when the mob is avoiding another mob.
    pub avoid_mob_sound: Option<String>,
    ///The next target position the entity chooses to avoid another entity will be chosen within this XZ Distance.
    pub avoid_target_xz: Option<i32>,
    ///The next target position the entity chooses to avoid another entity will be chosen within this Y Distance.
    pub avoid_target_y: Option<i32>,
    ///control_flags
    pub control_flags: Option<BehaviorAvoidMobTypeControlFlags>,
    ///The list of conditions another entity must meet to be a valid target to avoid.
    pub entity_types: Option<Vec<BehaviorAvoidMobTypeEntityTypes>>,
    ///If true, visbility between this entity and the mob type will not be checked.
    pub ignore_visibility: Option<bool>,
    ///Whether or not to ignore direct line of sight while this entity is running away from other specified entities.
    pub ignore_visibilty: Option<bool>,
    ///Maximum distance to look for an avoid target for the entity.
    pub max_dist: Option<f32>,
    ///How many blocks away from its avoid target the entity must be for it to stop fleeing from the avoid target.
    pub max_flee: Option<f32>,
    ///Event that is triggered when escaping from a mob.
    pub on_escape_event: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorAvoidMobTypePriority>,
    ///Percent chance this entity will stop avoiding another entity based on that entity's strength, where 1.0 = 100%.
    pub probability_per_strength: Option<f32>,
    ///Determine if we should remove target when fleeing or not.
    pub remove_target: Option<bool>,
    ///The range of time in seconds to randomly wait before playing the sound again.
    pub sound_interval: Option<crate::types::RangeOrVal<f32>>,
    ///How many blocks within range of its avoid target the entity must be for it to begin sprinting away from the avoid target.
    pub sprint_distance: Option<f32>,
    ///Multiplier for sprint speed. 1.0 means keep the regular speed, while higher numbers make the sprint speed faster.
    pub sprint_speed_multiplier: Option<f32>,
    ///Multiplier for walking speed. 1.0 means keep the regular speed, while higher numbers make the walking speed faster.
    pub walk_speed_multiplier: Option<f32>,
}
impl Default for BehaviorAvoidMobType {
    fn default() -> Self {
        Self {
            avoid_mob_sound: Some("undefined".to_string()),
            avoid_target_xz: Some(16i32),
            avoid_target_y: Some(7i32),
            control_flags: Some(BehaviorAvoidMobTypeControlFlags {}),
            entity_types: Some(vec![]),
            ignore_visibility: Some(false),
            ignore_visibilty: None,
            max_dist: Some(3f32),
            max_flee: Some(10f32),
            on_escape_event: Some(crate::types::BedrockValue::Object(
                std::collections::HashMap::from([
                    (
                        "event".to_string(),
                        crate::types::BedrockValue::String("".to_string()),
                    ),
                    (
                        "filters".to_string(),
                        crate::types::BedrockValue::Object(std::collections::HashMap::from([
                            ("AND".to_string(), crate::types::BedrockValue::Null),
                            ("NOT".to_string(), crate::types::BedrockValue::Null),
                            ("OR".to_string(), crate::types::BedrockValue::Null),
                            ("all".to_string(), crate::types::BedrockValue::Null),
                            ("all_of".to_string(), crate::types::BedrockValue::Null),
                            ("any".to_string(), crate::types::BedrockValue::Null),
                            ("any_of".to_string(), crate::types::BedrockValue::Null),
                            ("none_of".to_string(), crate::types::BedrockValue::Null),
                        ])),
                    ),
                    (
                        "target".to_string(),
                        crate::types::BedrockValue::String("self".to_string()),
                    ),
                ]),
            )),
            priority: Some(BehaviorAvoidMobTypePriority {}),
            probability_per_strength: Some(1f32),
            remove_target: Some(false),
            sound_interval: Some(crate::types::RangeOrVal::Range {
                min: 3f32,
                max: 8f32,
            }),
            sprint_distance: Some(7f32),
            sprint_speed_multiplier: Some(1f32),
            walk_speed_multiplier: Some(1f32),
        }
    }
}
