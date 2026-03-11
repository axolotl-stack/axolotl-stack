use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorHurtByTargetEntityTypes {
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
impl Default for BehaviorHurtByTargetEntityTypes {
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
pub struct BehaviorHurtByTargetPriority {}
impl Default for BehaviorHurtByTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.hurt_by_target`. Allows the mob to target another mob that hurts them.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorHurtByTarget {
    ///If true, nearby mobs of the same type will be alerted about the damage.
    pub alert_same_type: Option<bool>,
    ///List of entity types that this mob can target if they hurt their owner.
    pub entity_types: Option<Vec<BehaviorHurtByTargetEntityTypes>>,
    ///If true, the mob will hurt its owner and other mobs with the same owner as itself.
    pub hurt_owner: Option<bool>,
    ///priority
    pub priority: Option<BehaviorHurtByTargetPriority>,
}
impl Default for BehaviorHurtByTarget {
    fn default() -> Self {
        Self {
            alert_same_type: Some(false),
            entity_types: None,
            hurt_owner: Some(false),
            priority: None,
        }
    }
}
