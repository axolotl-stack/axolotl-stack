//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:skeleton_horse`
pub struct SkeletonHorse;
impl SkeletonHorse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:skeleton_horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:skeleton_horse`
#[derive(Bundle, Clone)]
pub struct SkeletonHorseBundle {
    pub ambient_sound_interval: super::super::components::AmbientSoundInterval,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_player_ride_tamed: super::super::components::BehaviorPlayerRideTamed,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_power_jump: super::super::components::CanPowerJump,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub health: super::super::components::Health,
    pub horse_jump_strength: super::super::components::HorseJumpStrength,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub input_ground_controlled: super::super::components::InputGroundControlled,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub is_tamed: super::super::components::IsTamed,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub type_family: super::super::components::TypeFamily,
    pub underwater_movement: super::super::components::UnderwaterMovement,
}
/// Spawn a new `minecraft:skeleton_horse` entity with default Bedrock components
pub fn spawn_skeleton_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(SkeletonHorseBundle {
            ambient_sound_interval: super::super::components::AmbientSoundInterval {
                event_name: Some("ambient".to_string()),
                event_names: None,
                range: Some(16f32),
                value: 8f32,
            },
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {
                }),
                target_dist: Some(4f32),
                track_target: Some(true),
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(
                    vec![
                        "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                        .to_string()
                    ],
                ),
                force: Some(false),
                ignore_mob_damage: Some(false),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_player_ride_tamed: super::super::components::BehaviorPlayerRideTamed {
                priority: None,
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {
                }),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_power_jump: super::super::components::CanPowerJump,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
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
            health: super::super::components::Health {
                max: Some(15f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(15f32),
            },
            horse_jump_strength: super::super::components::HorseJumpStrength {
                value: crate::types::RangeOrVal::Range {
                    min: 0.4f32,
                    max: 1f32,
                },
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            input_ground_controlled: super::super::components::InputGroundControlled,
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            is_tamed: super::super::components::IsTamed,
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
                presets: Some(
                    vec![
                        LeashablePresets { filter : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("happy_ghast"
                        .to_string()))]))), hard_distance : None, max_distance : None,
                        rotation_adjustment : None, soft_distance : None, spring_type :
                        Some("quad_dampened".to_string()) }
                    ],
                ),
                soft_distance: Some(4f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.2f32),
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
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(true),
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
                can_path_over_water: Some(false),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(true),
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
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("default".to_string()),
                family_types: Some(
                    vec![
                        "player".to_string(), "skeleton".to_string(), "baby_zombie"
                        .to_string(), "baby_husk".to_string()
                    ],
                ),
                interact_text: Some("action.interact.ride.horse".to_string()),
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(false),
                rider_can_interact: Some(false),
                seat_count: Some(1i32),
                seats: Some(
                    vec![
                        RideableSeats { camera_relax_distance_smoothing : None,
                        lock_rider_rotation : None, max_rider_count : None,
                        min_rider_count : None, position : None, rotate_rider_by : None,
                        third_person_camera_radius : None }
                    ],
                ),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "skeletonhorse".to_string(), "undead".to_string(), "mob".to_string()
                ],
            },
            underwater_movement: super::super::components::UnderwaterMovement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.08f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkeletonHorseComponentGroup {
    LightningImmune,
    SkeletonHorseAdult,
    SkeletonHorseBaby,
    SkeletonHorseR5Upgrade,
    SkeletonTrap,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkeletonHorseEvent {
    EntitySpawned,
    SetTrap,
    SpringTrap,
}
