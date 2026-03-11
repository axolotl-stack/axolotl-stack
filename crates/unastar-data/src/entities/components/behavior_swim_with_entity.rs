use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimWithEntityEntityTypes {
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
impl Default for BehaviorSwimWithEntityEntityTypes {
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
pub struct BehaviorSwimWithEntityPriority {}
impl Default for BehaviorSwimWithEntityPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimWithEntitySpeedMultiplier {}
impl Default for BehaviorSwimWithEntitySpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swim_with_entity`. Allows the entity follow another entity. Both entities must be swimming and in water.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimWithEntity {
    ///The multiplier this entity's speed is modified by when matching another entity's direction.
    pub catch_up_multiplier: Option<f32>,
    ///Distance, from the entity being followed, at which this entity will speed up to reach that entity.
    pub catch_up_threshold: Option<f32>,
    ///Percent chance to stop following the current entity, if they're riding another entity or they're not swimming. 1.0 = 100%
    pub chance_to_stop: Option<f32>,
    ///Filters which determine what entites are valid to follow.
    pub entity_types: Option<Vec<BehaviorSwimWithEntityEntityTypes>>,
    ///Distance, from the entity being followed, at which this entity will try to match that entity's direction.
    pub match_direction_threshold: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSwimWithEntityPriority>,
    ///Radius around this entity to search for another entity to follow.
    pub search_range: Option<f32>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSwimWithEntitySpeedMultiplier>,
    ///Time (in seconds) between checks to determine if this entity should catch up to the entity being followed or match the direction of the entity being followed.
    pub state_check_interval: Option<f32>,
    ///Distance, from the entity being followed, at which this entity will stop following that entity.
    pub stop_distance: Option<f32>,
    ///Percent chance to start following another entity, if not already doing so. 1.0 = 100%
    pub success_rate: Option<f32>,
}
impl Default for BehaviorSwimWithEntity {
    fn default() -> Self {
        Self {
            catch_up_multiplier: Some(2.5f32),
            catch_up_threshold: Some(12f32),
            chance_to_stop: Some(0.0333f32),
            entity_types: None,
            match_direction_threshold: Some(2f32),
            priority: None,
            search_range: Some(20f32),
            speed_multiplier: Some(BehaviorSwimWithEntitySpeedMultiplier {}),
            state_check_interval: Some(0.5f32),
            stop_distance: Some(5f32),
            success_rate: Some(0.1f32),
        }
    }
}
