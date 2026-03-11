//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie_pigman`
pub struct ZombiePigman;
impl ZombiePigman {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_pigman";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_pigman`
#[derive(Bundle, Clone)]
pub struct ZombiePigmanBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stomp_turtle_egg: BehaviorStompTurtleEgg,
    pub behavior_use_kinetic_weapon: BehaviorUseKineticWeapon,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_pigman` entity with default Bedrock components
pub fn spawn_zombie_pigman(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombiePigmanBundle {
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(3i32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(2i32),
                speed_multiplier: Some(1.25f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(8i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stomp_turtle_egg: BehaviorStompTurtleEgg {
                goal_radius: Some(1.14f32),
                interval: Some(20i32),
                priority: Some(6i32),
                search_count: None,
                search_height: Some(2i32),
                search_range: Some(10i32),
                speed_multiplier: Some(1f32),
            },
            behavior_use_kinetic_weapon: BehaviorUseKineticWeapon {
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
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            fire_immune: FireImmune,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: Loot {
                table: "loot_tables/entities/zombie_pigman.json".to_string(),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombiePigmanComponentGroup {
    PigZombieAdult,
    PigZombieAngry,
    PigZombieBaby,
    PigZombieCalm,
    StriderJockey,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombiePigmanEvent {
    BecomeAngry,
    EntitySpawned,
    EntityTransformed,
    OnCalm,
    SpawnAsStriderJockey,
    SpawnAdult,
    SpawnBaby,
}
