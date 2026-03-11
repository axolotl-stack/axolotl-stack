//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:polar_bear`
pub struct PolarBear;
impl PolarBear {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:polar_bear";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:polar_bear`
#[derive(Bundle, Clone)]
pub struct PolarBearBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub water_movement: super::super::components::WaterMovement,
}
/// Spawn a new `minecraft:polar_bear` entity with default Bedrock components
pub fn spawn_polar_bear(commands: &mut Commands) -> Entity {
    commands
        .spawn(PolarBearBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: Some(6f32),
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(vec![
                    "campfire".to_string(),
                    "fire".to_string(),
                    "fire_tick".to_string(),
                    "freezing".to_string(),
                    "lightning".to_string(),
                    "lava".to_string(),
                    "magma".to_string(),
                    "temperature".to_string(),
                    "soul_campfire".to_string(),
                ]),
                force: Some(false),
                ignore_mob_damage: Some(true),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.4f32),
                width: Some(1.4f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: Some(DespawnDespawnFromDistance {
                    max_distance: None,
                    min_distance: None,
                }),
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: None,
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(48f32),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(30f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable: super::super::components::Leashable {
                can_be_cut: Some(true),
                can_be_stolen: Some(false),
                hard_distance: Some(6f32),
                max_distance: Some(0f32),
                on_leash: None,
                on_unleash: None,
                on_unleash_interact_only: Some(false),
                presets: None,
                soft_distance: Some(4f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_walk: super::super::components::NavigationWalk {
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(false),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_float: None,
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["polarbear".to_string(), "mob".to_string()],
            },
            water_movement: super::super::components::WaterMovement {
                drag_factor: Some(0.98f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolarBearComponentGroup {
    Adult,
    AdultHostile,
    AdultWild,
    Baby,
    BabyScared,
    BabyWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolarBearEvent {
    AgeableGrowUp,
    BabyOnCalm,
    EntityBorn,
    EntitySpawned,
    OnAnger,
    OnCalm,
    OnScared,
}
