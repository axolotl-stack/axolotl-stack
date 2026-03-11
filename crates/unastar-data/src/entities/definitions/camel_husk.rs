//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:camel_husk`
pub struct CamelHusk;
impl CamelHusk {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:camel_husk";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:camel_husk`
#[derive(Bundle, Clone)]
pub struct CamelHuskBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_look_around_and_sit:
        super::super::components::BehaviorRandomLookAroundAndSit,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub experience_reward: super::super::components::ExperienceReward,
    pub healable: super::super::components::Healable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub inventory: super::super::components::Inventory,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub is_tamed: super::super::components::IsTamed,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub rideable: super::super::components::Rideable,
    pub type_family: super::super::components::TypeFamily,
    pub variable_max_auto_step: super::super::components::VariableMaxAutoStep,
}
/// Spawn a new `minecraft:camel_husk` entity with default Bedrock components
pub fn spawn_camel_husk(commands: &mut Commands) -> Entity {
    commands
        .spawn(CamelHuskBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(1f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(2f32),
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
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {}),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_random_look_around_and_sit:
                super::super::components::BehaviorRandomLookAroundAndSit {
                    continue_if_leashed: Some(true),
                    continue_sitting_on_reload: Some(true),
                    max_angle_of_view_horizontal: Some(30f32),
                    max_look_count: Some(5i32),
                    max_look_time: Some(100i32),
                    min_angle_of_view_horizontal: Some(-30f32),
                    min_look_count: Some(2i32),
                    min_look_time: Some(80i32),
                    priority: Some(BehaviorRandomLookAroundAndSitPriority {}),
                    probability: Some(0.001f32),
                    random_look_around_cooldown: Some(5i32),
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
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(2.375f32),
                width: Some(1.7f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: Some("fall".to_string()),
                    damage_modifier: Some(-4f32),
                    damage_multiplier: None,
                    deals_damage: Some("yes".to_string()),
                    on_damage: None,
                    on_damage_sound_event: None,
                }]),
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
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Array(vec![
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:all_riders_dismounted".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "all_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "domain".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:has_rider_mounted".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "bool_property".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "rider_count".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(0i64),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                        ),
                    ])),
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:rider_mounted".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "all_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "domain".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:has_rider_mounted".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "bool_property".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(false),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        ">".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "rider_count".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(0i64),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                        ),
                    ])),
                ])),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Expr("Math.Random(1,7)".to_string())),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? Math.Random(1,3) : 0".to_string(),
                )),
            },
            healable: super::super::components::Healable {
                filters: None,
                force_use: Some(false),
                items: Some(vec![HealableItems {
                    effects: None,
                    filters: None,
                    heal_amount: Some(2i32),
                    item: Some(crate::types::BedrockValue::String(
                        "rabbit_foot".to_string(),
                    )),
                    result_item: None,
                }]),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(32f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("horse".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            is_tamed: super::super::components::IsTamed,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/camel_husk.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.09f32),
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
            rideable: super::super::components::Rideable {
                controlling_seat: Some(0i32),
                crouching_skip_interact: Some(true),
                dismount_mode: Some("default".to_string()),
                family_types: Some(vec![
                    "player".to_string(),
                    "parched".to_string(),
                    "husk_rider".to_string(),
                ]),
                interact_text: Some("action.interact.ride.horse".to_string()),
                on_rider_enter_event: None,
                on_rider_exit_event: None,
                passenger_max_width: Some(0f32),
                pull_in_entities: Some(false),
                rider_can_interact: Some(false),
                seat_count: Some(2i32),
                seats: Some(vec![
                    RideableSeats {
                        camera_relax_distance_smoothing: None,
                        lock_rider_rotation: None,
                        max_rider_count: Some(2i32),
                        min_rider_count: Some(0i32),
                        position: None,
                        rotate_rider_by: None,
                        third_person_camera_radius: None,
                    },
                    RideableSeats {
                        camera_relax_distance_smoothing: None,
                        lock_rider_rotation: None,
                        max_rider_count: Some(2i32),
                        min_rider_count: Some(1i32),
                        position: None,
                        rotate_rider_by: None,
                        third_person_camera_radius: None,
                    },
                ]),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "camel_husk".to_string(),
                    "mob".to_string(),
                    "undead".to_string(),
                ],
            },
            variable_max_auto_step: super::super::components::VariableMaxAutoStep {
                base_value: Some(1.5625f32),
                controlled_value: Some(1.5625f32),
                jump_prevented_value: Some(0.5625f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelHuskComponentGroup {
    CamelHuskSaddled,
    CamelHuskSitting,
    CamelHuskStanding,
    CamelHuskWithHostileRider,
    CamelHuskWithNoHostileRider,
    CamelHuskWithNoRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelHuskEvent {
    AllRidersDismounted,
    CamelHuskSaddled,
    CamelHuskUnsaddled,
    EntitySpawned,
    RiderMounted,
    SpawnWithRider,
    StartSitting,
    StopSitting,
}
