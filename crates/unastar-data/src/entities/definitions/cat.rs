//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:cat`
pub struct Cat;
impl Cat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:cat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:cat`
#[derive(Bundle, Clone)]
pub struct CatBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_leap_at_target: BehaviorLeapAtTarget,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_ocelotattack: BehaviorOcelotattack,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub dweller: Dweller,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:cat` entity with default Bedrock components
pub fn spawn_cat(commands: &mut Commands) -> Entity {
    commands
        .spawn(CatBundle {
            balloonable: Balloonable {
                mass: Some(0.6f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_leap_at_target: BehaviorLeapAtTarget {
                must_be_on_ground: Some(true),
                priority: Some(3i32),
                set_persistent: Some(false),
                target_dist: Some(0.3f32),
                yd: Some(0f32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(1i32),
                speed_multiplier: Some(1.25f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_ocelotattack: BehaviorOcelotattack {
                cooldown_time: Some(1f32),
                max_distance: Some(15f32),
                max_sneak_range: Some(15f32),
                max_sprint_range: Some(4f32),
                priority: None,
                reach_multiplier: Some(2f32),
                sneak_speed_multiplier: Some(0.6f32),
                sprint_speed_multiplier: Some(1.33f32),
                walk_speed_multiplier: Some(0.8f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(8i32),
                speed_multiplier: Some(0.8f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.7f32),
                width: Some(0.6f32),
            },
            dweller: Dweller {
                can_find_poi: None,
                can_migrate: None,
                dweller_role: None,
                dwelling_bounds_tolerance: None,
                dwelling_type: None,
                first_founding_reward: None,
                preferred_profession: None,
                update_interval_base: None,
                update_interval_variant: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
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
pub enum CatComponentGroup {
    CatAdult,
    CatBaby,
    CatBlack,
    CatBritish,
    CatCalico,
    CatGiftForOwner,
    CatJellie,
    CatPersian,
    CatRagdoll,
    CatRed,
    CatSiamese,
    CatTabby,
    CatTame,
    CatTuxedo,
    CatWhite,
    CatWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatEvent {
    AgeableGrowUp,
    CatGiftedOwner,
    EntityBorn,
    EntitySpawned,
    OnTame,
    PetSleptWithOwner,
    SpawnFromVillage,
    SpawnMidnightCat,
    SpawnTameAdult,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildBaby,
}
