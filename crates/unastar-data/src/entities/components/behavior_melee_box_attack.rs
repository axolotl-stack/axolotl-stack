use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMeleeBoxAttackControlFlags {}
impl Default for BehaviorMeleeBoxAttackControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMeleeBoxAttackPriority {}
impl Default for BehaviorMeleeBoxAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMeleeBoxAttackSpeedMultiplier {}
impl Default for BehaviorMeleeBoxAttackSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.melee_box_attack`. Permits an entity to deal damage through a melee attack with reach calculations based on bounding boxes.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorMeleeBoxAttack {
    ///Allows the entity to use this attack behavior, only once EVER.
    pub attack_once: Option<bool>,
    ///Defines the entity types this entity will attack.
    pub attack_types: Option<String>,
    ///If the entity is on fire, this allows the entity's target to catch on fire after being hi
    pub can_spread_on_fire: Option<bool>,
    ///control_flags
    pub control_flags: Option<BehaviorMeleeBoxAttackControlFlags>,
    ///Cooldown time (in seconds) between attacks.
    pub cooldown_time: Option<f32>,
    ///The attack reach of the entity will be a box with the size of the entity's bounds increased by this value in all horizontal directions.
    pub horizontal_reach: Option<f32>,
    ///Time (in seconds) to add to attack path recalculation when the target is beyond the "path_inner_boundary".
    pub inner_boundary_time_increase: Option<f32>,
    ///Unused. No effect on "minecraft:behavior.melee_attack".
    pub max_dist: Option<f32>,
    ///Maximum base time (in seconds) to recalculate new attack path to target (before increases applied).
    pub max_path_time: Option<f32>,
    ///Field of view (in degrees) when using the sensing component to detect an attack target.
    pub melee_fov: Option<f32>,
    ///Minimum base time (in seconds) to recalculate new attack path to target (before increases applied).
    pub min_path_time: Option<f32>,
    ///Defines the event to trigger when this entity successfully attacks.
    pub on_attack: Option<crate::types::BedrockValue>,
    ///Defines the event to trigger when this entity successfully kills a target.
    pub on_kill: Option<crate::types::BedrockValue>,
    ///Time (in seconds) to add to attack path recalculation when the target is beyond the "path_outer_boundary".
    pub outer_boundary_time_increase: Option<f32>,
    ///Time (in seconds) to add to attack path recalculation when this entity cannot move along the current path.
    pub path_fail_time_increase: Option<f32>,
    ///Distance at which to increase attack path recalculation by "inner_boundary_tick_increase".
    pub path_inner_boundary: Option<f32>,
    ///Distance at which to increase attack path recalculation by "outer_boundary_tick_increase".
    pub path_outer_boundary: Option<f32>,
    ///priority
    pub priority: Option<BehaviorMeleeBoxAttackPriority>,
    ///This entity will have a 1 in N chance to stop it's current attack, where N = "random_stop_interval".
    pub random_stop_interval: Option<i32>,
    ///Used with the base size of the entity to determine minimum target-distance before trying to deal attack damage.
    pub reach_multiplier: Option<f32>,
    ///Toggles (on/off) the need to have a full path from the entity to the target when using this melee attack behavior.
    pub require_complete_path: Option<bool>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMeleeBoxAttackSpeedMultiplier>,
    ///Unused. No effect on "minecraft:behavior.melee_attack".
    pub target_dist: Option<f32>,
    ///Allows the entity to track the attack target, even if the entity has no sensing.
    pub track_target: Option<bool>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorMeleeBoxAttack {
    fn default() -> Self {
        Self {
            attack_once: Some(false),
            attack_types: None,
            can_spread_on_fire: Some(false),
            control_flags: None,
            cooldown_time: Some(1f32),
            horizontal_reach: Some(0.8f32),
            inner_boundary_time_increase: Some(0.25f32),
            max_dist: None,
            max_path_time: Some(0.55f32),
            melee_fov: Some(90f32),
            min_path_time: Some(0.2f32),
            on_attack: None,
            on_kill: None,
            outer_boundary_time_increase: Some(0.5f32),
            path_fail_time_increase: Some(0.75f32),
            path_inner_boundary: Some(16f32),
            path_outer_boundary: Some(32f32),
            priority: None,
            random_stop_interval: Some(0i32),
            reach_multiplier: None,
            require_complete_path: Some(false),
            set_persistent: None,
            speed_multiplier: Some(BehaviorMeleeBoxAttackSpeedMultiplier {}),
            target_dist: None,
            track_target: Some(false),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
