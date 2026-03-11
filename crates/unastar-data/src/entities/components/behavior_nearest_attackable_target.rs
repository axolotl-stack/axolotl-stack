use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorNearestAttackableTargetControlFlags {}
impl Default for BehaviorNearestAttackableTargetControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorNearestAttackableTargetEntityTypes {
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
impl Default for BehaviorNearestAttackableTargetEntityTypes {
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
pub struct BehaviorNearestAttackableTargetPriority {}
impl Default for BehaviorNearestAttackableTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.nearest_attackable_target`. Allows an entity to attack the closest target within a given subset of specific target types.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorNearestAttackableTarget {
    ///Time range (in seconds) between searching for an attack target, range is in (0, `attack_interval`]. Only used if `attack_interval` is greater than 0, otherwise `scan_interval` is used.
    pub attack_interval: Option<crate::types::BedrockValue>,
    ///Alias for `attack_interval`; provides the same functionality as `attack_interval`.
    pub attack_interval_min: Option<f32>,
    ///If true, this entity can attack its owner.
    pub attack_owner: Option<bool>,
    ///control_flags
    pub control_flags: Option<BehaviorNearestAttackableTargetControlFlags>,
    ///Filters which types of targets are valid for this entity
    pub entity_types: Option<Vec<BehaviorNearestAttackableTargetEntityTypes>>,
    ///If true, this entity requires a path to the target.
    pub must_reach: Option<bool>,
    ///Determines if target-validity requires this entity to be in range only, or both in range and in sight.
    pub must_see: Option<bool>,
    ///Time (in seconds) the target must not be seen by this entity to become invalid. Used only if `must_see` is true.
    pub must_see_forget_duration: Option<f32>,
    ///Time (in seconds) this entity can continue attacking the target after the target is no longer valid.
    pub persist_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorNearestAttackableTargetPriority>,
    ///Allows the attacking entity to update the nearest target, otherwise a target is only reselected after each `scan_interval` or `attack_interval`.
    pub reselect_targets: Option<bool>,
    ///If `attack_interval` is 0 or isn't declared, then between attacks: scanning for a new target occurs every amount of ticks equal to `scan_interval`, minimum value is 1. Values under 10 can affect performance.
    pub scan_interval: Option<i32>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///Probability (0.0 to 1.0) that this entity will accept a found target. Checked each time a valid target is found during scanning.
    pub target_acquisition_probability: Option<f32>,
    ///Multiplied with the target's armor coverage percentage to modify `max_dist` when detecting an invisible target.
    pub target_invisible_multiplier: Option<f32>,
    ///Maximum vertical target-search distance, if it's greater than the target type's `max_dist`. A negative value defaults to `entity_types` greatest `max_dist`.
    pub target_search_height: Option<f32>,
    ///Multiplied with the target type's `max_dist` when trying to detect a sneaking target.
    pub target_sneak_visibility_multiplier: Option<f32>,
    ///Maximum distance this entity can be from the target when following it, otherwise the target becomes invalid. This value is only used if the entity doesn't declare `minecraft:follow_range`.
    pub within_radius: Option<f32>,
}
impl Default for BehaviorNearestAttackableTarget {
    fn default() -> Self {
        Self {
            attack_interval: Some(crate::types::BedrockValue::Object(
                std::collections::HashMap::from([
                    ("max".to_string(), crate::types::BedrockValue::Integer(0i64)),
                    ("min".to_string(), crate::types::BedrockValue::Integer(0i64)),
                ]),
            )),
            attack_interval_min: None,
            attack_owner: Some(false),
            control_flags: Some(BehaviorNearestAttackableTargetControlFlags {}),
            entity_types: Some(vec![]),
            must_reach: Some(false),
            must_see: Some(false),
            must_see_forget_duration: Some(3f32),
            persist_time: Some(0f32),
            priority: Some(BehaviorNearestAttackableTargetPriority {}),
            reselect_targets: Some(false),
            scan_interval: Some(10i32),
            set_persistent: Some(false),
            target_acquisition_probability: Some(1f32),
            target_invisible_multiplier: Some(0.7f32),
            target_search_height: Some(-1f32),
            target_sneak_visibility_multiplier: Some(0.8f32),
            within_radius: Some(0f32),
        }
    }
}
