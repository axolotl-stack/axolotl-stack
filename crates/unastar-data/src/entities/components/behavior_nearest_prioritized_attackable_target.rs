use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorNearestPrioritizedAttackableTargetEntityTypes {
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
impl Default for BehaviorNearestPrioritizedAttackableTargetEntityTypes {
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
pub struct BehaviorNearestPrioritizedAttackableTargetPriority {}
impl Default for BehaviorNearestPrioritizedAttackableTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.nearest_prioritized_attackable_target`. Allows the mob to check for and pursue the nearest valid target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorNearestPrioritizedAttackableTarget {
    ///Time in seconds before selecting a target.
    pub attack_interval: Option<i32>,
    ///The amount of time in seconds that the mob has to wait before selecting a target of the same type again
    pub cooldown: Option<f32>,
    ///List of entity types that this mob considers valid targets
    pub entity_types: Option<Vec<BehaviorNearestPrioritizedAttackableTargetEntityTypes>>,
    ///If true, only entities that this mob can path to can be selected as targets.
    pub must_reach: Option<bool>,
    ///If true, only entities in this mob's viewing range can be selected as targets.
    pub must_see: Option<bool>,
    ///Determines the amount of time in seconds that this mob will look for a target before forgetting about it and looking for a new one when the target isn't visible any more.
    pub must_see_forget_duration: Option<f32>,
    ///Time in seconds for a valid target to stay targeted when it becomes and invalid target.
    pub persist_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorNearestPrioritizedAttackableTargetPriority>,
    ///If true, the mob will stop being targeted if it stops meeting any conditions.
    pub reevaluate_description: Option<bool>,
    ///If true, the target will change to the current closest entity whenever a different entity is closer.
    pub reselect_targets: Option<bool>,
    ///How many ticks to wait between scanning for a target.
    pub scan_interval: Option<i32>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///Height in blocks to search for a target mob. -1.0f means the height does not matter.
    pub target_search_height: Option<f32>,
    ///Distance in blocks that the target can be within to launch an attack.
    pub within_radius: Option<f32>,
}
impl Default for BehaviorNearestPrioritizedAttackableTarget {
    fn default() -> Self {
        Self {
            attack_interval: Some(0i32),
            cooldown: Some(0f32),
            entity_types: None,
            must_reach: Some(false),
            must_see: Some(false),
            must_see_forget_duration: Some(3f32),
            persist_time: Some(0f32),
            priority: Some(BehaviorNearestPrioritizedAttackableTargetPriority {}),
            reevaluate_description: None,
            reselect_targets: Some(false),
            scan_interval: Some(10i32),
            set_persistent: Some(false),
            target_search_height: Some(-1f32),
            within_radius: Some(0f32),
        }
    }
}
