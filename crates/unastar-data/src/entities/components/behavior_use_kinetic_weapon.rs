use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorUseKineticWeaponPriority {}
impl Default for BehaviorUseKineticWeaponPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorUseKineticWeaponSpeedMultiplier {}
impl Default for BehaviorUseKineticWeaponSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.use_kinetic_weapon`. Allows a mob to make use of items with a "minecraft:kinetic_weapon" item component. The mob will approach the target before using the weapon and charging with it. If the target gets too close, the mob will retreat and reposition before charging again. Once all "max_duration"s in the item's "minecraft:kinetic_weapon" component have elapsed, the mob goes on cooldown and retreats before approaching again
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorUseKineticWeapon {
    ///The distance to the target within which the mob begins using its kinetic weapon
    pub approach_distance: Option<f32>,
    ///Allows the entity to use this attack behavior, only once EVER.
    pub attack_once: Option<bool>,
    ///The distance the mob retreats to after all of the item's "minecraft:kinetic_weapon" component's "max_duration" values have elapsed
    pub cooldown_distance: Option<f32>,
    ///Multiplier applied to the mob's movement speed while on cooldown
    pub cooldown_speed_multiplier: Option<f32>,
    ///Cooldown time (in seconds) between attacks.
    pub cooldown_time: Option<f32>,
    ///Allows a mob to override its mount's navigation behavior with the one defined by this goal. Requires the mount to be running the "minecraft:behavior.mount_pathing" goal, whose default behavior will be ignored
    pub hijack_mount_navigation: Option<bool>,
    ///Maximum base time (in seconds) to recalculate new attack path to target (before increases applied).
    pub max_path_time: Option<f32>,
    ///Field of view (in degrees) when using the sensing component to detect an attack target.
    pub melee_fov: Option<f32>,
    ///Minimum base time (in seconds) to recalculate new attack path to target (before increases applied).
    pub min_path_time: Option<f32>,
    ///Time (in seconds) to add to attack path recalculation when the target is beyond the "path_outer_boundary".
    pub outer_boundary_time_increase: Option<f32>,
    ///Time (in seconds) to add to attack path recalculation when this entity cannot move along the current path.
    pub path_fail_time_increase: Option<f32>,
    ///Distance at which to increase attack path recalculation by "inner_boundary_tick_increase".
    pub path_inner_boundary: Option<f32>,
    ///Distance at which to increase attack path recalculation by "outer_boundary_tick_increase".
    pub path_outer_boundary: Option<f32>,
    ///priority
    pub priority: Option<BehaviorUseKineticWeaponPriority>,
    ///This entity will have a 1 in N chance to stop it's current attack, where N = "random_stop_interval".
    pub random_stop_interval: Option<i32>,
    ///The distance the mob retreats to once the target is closer than the midpoint of the item's "minecraft:kinetic_weapon" component's minimum and maximum "reach"
    pub reposition_distance: Option<f32>,
    ///Multiplier applied to the mob's movement speed while repositioning
    pub reposition_speed_multiplier: Option<f32>,
    ///Specifies whether a full navigation path from the mob to the target is required
    pub require_complete_path: Option<bool>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorUseKineticWeaponSpeedMultiplier>,
    ///Allows the entity to track the attack target, even if the entity has no sensing.
    pub track_target: Option<bool>,
    ///Multiplier applied to each "min_speed" and "min_relative_speed" condition in the item's "minecraft:kinetic_weapon" component
    pub weapon_min_speed_multiplier: Option<f32>,
    ///Multiplier applied to the item's "minecraft:kinetic_weapon" component's "reach"
    pub weapon_reach_multiplier: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorUseKineticWeapon {
    fn default() -> Self {
        Self {
            approach_distance: Some(8f32),
            attack_once: Some(false),
            cooldown_distance: Some(0f32),
            cooldown_speed_multiplier: Some(1f32),
            cooldown_time: Some(1f32),
            hijack_mount_navigation: Some(false),
            max_path_time: Some(0.55f32),
            melee_fov: Some(90f32),
            min_path_time: Some(0.2f32),
            outer_boundary_time_increase: Some(0.5f32),
            path_fail_time_increase: Some(0.75f32),
            path_inner_boundary: Some(16f32),
            path_outer_boundary: Some(32f32),
            priority: Some(BehaviorUseKineticWeaponPriority {}),
            random_stop_interval: Some(0i32),
            reposition_distance: Some(0f32),
            reposition_speed_multiplier: Some(1f32),
            require_complete_path: Some(false),
            speed_multiplier: Some(BehaviorUseKineticWeaponSpeedMultiplier {}),
            track_target: Some(false),
            weapon_min_speed_multiplier: Some(1f32),
            weapon_reach_multiplier: Some(1f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
