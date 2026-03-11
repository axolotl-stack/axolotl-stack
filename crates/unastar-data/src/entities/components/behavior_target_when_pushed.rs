use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTargetWhenPushedEntityTypes {
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
impl Default for BehaviorTargetWhenPushedEntityTypes {
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
pub struct BehaviorTargetWhenPushedPriority {}
impl Default for BehaviorTargetWhenPushedPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.target_when_pushed`. Allows an entity to select a valid target entity that pushed it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTargetWhenPushed {
    ///The list of conditions the other entity must meet to be a valid target.
    pub entity_types: Option<Vec<BehaviorTargetWhenPushedEntityTypes>>,
    ///Probability that the entity will target the entity that pushed it.
    pub percent_chance: Option<f32>,
    ///priority
    pub priority: Option<BehaviorTargetWhenPushedPriority>,
}
impl Default for BehaviorTargetWhenPushed {
    fn default() -> Self {
        Self {
            entity_types: None,
            percent_chance: None,
            priority: None,
        }
    }
}
