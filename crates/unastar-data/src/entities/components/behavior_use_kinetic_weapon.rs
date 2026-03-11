use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.use_kinetic_weapon`. Allows a mob to make use of items with a "minecraft:kinetic_weapon" item component. The mob will approach the target before using the weapon and charging with it. If the target gets too close, the mob will retreat and reposition before charging again. Once all "max_duration"s in the item's "minecraft:kinetic_weapon" component have elapsed, the mob goes on cooldown and retreats before approaching again
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorUseKineticWeapon {
    /// approach_distance
    pub approach_distance: Option<f32>,
    /// attack_once
    pub attack_once: Option<bool>,
    /// cooldown_distance
    pub cooldown_distance: Option<f32>,
    /// cooldown_speed_multiplier
    pub cooldown_speed_multiplier: Option<f32>,
    /// cooldown_time
    pub cooldown_time: Option<f32>,
    /// hijack_mount_navigation
    pub hijack_mount_navigation: Option<bool>,
    /// max_path_time
    pub max_path_time: Option<f32>,
    /// melee_fov
    pub melee_fov: Option<f32>,
    /// min_path_time
    pub min_path_time: Option<f32>,
    /// outer_boundary_time_increase
    pub outer_boundary_time_increase: Option<f32>,
    /// path_fail_time_increase
    pub path_fail_time_increase: Option<f32>,
    /// path_inner_boundary
    pub path_inner_boundary: Option<f32>,
    /// path_outer_boundary
    pub path_outer_boundary: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// random_stop_interval
    pub random_stop_interval: Option<i32>,
    /// reposition_distance
    pub reposition_distance: Option<f32>,
    /// reposition_speed_multiplier
    pub reposition_speed_multiplier: Option<f32>,
    /// require_complete_path
    pub require_complete_path: Option<bool>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// track_target
    pub track_target: Option<bool>,
    /// weapon_min_speed_multiplier
    pub weapon_min_speed_multiplier: Option<f32>,
    /// weapon_reach_multiplier
    pub weapon_reach_multiplier: Option<f32>,
    /// x_max_rotation
    pub x_max_rotation: Option<f32>,
    /// y_max_head_rotation
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorUseKineticWeapon {
    fn default() -> Self {
        Self {
            approach_distance: Some(8f32),
            attack_once: Some(false),
            cooldown_distance: None,
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
            priority: Some(0i32),
            random_stop_interval: Some(0i32),
            reposition_distance: None,
            reposition_speed_multiplier: Some(1f32),
            require_complete_path: Some(false),
            speed_multiplier: Some(1f32),
            track_target: Some(false),
            weapon_min_speed_multiplier: Some(1f32),
            weapon_reach_multiplier: Some(1f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
