//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:breeze`
pub struct Breeze;
impl Breeze {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:breeze";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:breeze`
#[derive(Bundle, Clone)]
pub struct BreezeBundle {
    pub behavior_fire_at_target: super::super::components::BehaviorFireAtTarget,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_jump_around_target: super::super::components::BehaviorJumpAroundTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_move_around_target: super::super::components::BehaviorMoveAroundTarget,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub reflect_projectiles: super::super::components::ReflectProjectiles,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:breeze` entity with default Bedrock components
pub fn spawn_breeze(commands: &mut Commands) -> Entity {
    commands
        .spawn(BreezeBundle {
            behavior_fire_at_target: super::super::components::BehaviorFireAtTarget {
                attack_cooldown: Some(0.5f32),
                attack_range: None,
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "all_of".to_string(),
                        crate::types::BedrockValue::Array(vec![
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_navigating".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(false)),
                            ])),
                        ]),
                    )]),
                )),
                max_head_rotation_x: Some(30f32),
                max_head_rotation_y: Some(30f32),
                owner_anchor: Some(2i32),
                owner_offset: Some(vec![0f32, 0f32, 0f32]),
                post_shoot_delay: Some(0.2f32),
                pre_shoot_delay: Some(0.75f32),
                priority: Some(BehaviorFireAtTargetPriority {}),
                projectile_def: Some("minecraft:breeze_wind_charge_projectile".to_string()),
                ranged_fov: Some(90f32),
                target_anchor: Some(0i32),
                target_offset: Some(vec![0f32, 0f32, 0f32]),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: Some(vec![BehaviorHurtByTargetEntityTypes {
                    check_if_outnumbered: None,
                    cooldown: None,
                    filters: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([(
                            "all_of".to_string(),
                            crate::types::BedrockValue::Array(vec![
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "skeleton".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String("stray".to_string()),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "zombie".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String("husk".to_string()),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "spider".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "cavespider".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String("slime".to_string()),
                                        ),
                                    ]),
                                ),
                            ]),
                        )]),
                    )),
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
                }]),
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_jump_around_target: super::super::components::BehaviorJumpAroundTarget {
                check_collision: Some(false),
                entity_bounding_box_scale: Some(crate::types::BedrockValue::Float(0.7f64)),
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "all_of".to_string(),
                        crate::types::BedrockValue::Array(vec![
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "any_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "in_water".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(true),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "on_ground".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(true),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_riding".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(false)),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("in_lava".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(false)),
                            ])),
                        ]),
                    )]),
                )),
                jump_angles: Some(vec![40f32, 55f32, 60f32, 75f32, 80f32]),
                jump_cooldown_duration: Some(0.5f32),
                jump_cooldown_when_hurt_duration: Some(0.1f32),
                landing_distance_from_target: None,
                landing_position_spread_degrees: Some(90i32),
                last_hurt_duration: Some(2f32),
                line_of_sight_obstruction_height_ignore: Some(4i32),
                max_jump_velocity: Some(1.4f32),
                prepare_jump_duration: Some(0.5f32),
                priority: Some(BehaviorJumpAroundTargetPriority {}),
                required_vertical_space: Some(4i32),
                snap_to_surface_block_range: Some(10i32),
                valid_distance_to_target: None,
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(16f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_move_around_target: super::super::components::BehaviorMoveAroundTarget {
                destination_pos_search_spread_degrees: None,
                destination_pos_spread_degrees: Some(360f32),
                destination_position_range: None,
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "all_of".to_string(),
                        crate::types::BedrockValue::Array(vec![
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("on_ground".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "operator".to_string(),
                                    crate::types::BedrockValue::String("<=".to_string()),
                                ),
                                (
                                    "subject".to_string(),
                                    crate::types::BedrockValue::String("self".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String(
                                        "target_distance".to_string(),
                                    ),
                                ),
                                (
                                    "value".to_string(),
                                    crate::types::BedrockValue::Float(24f64),
                                ),
                            ])),
                        ]),
                    )]),
                )),
                height_difference_limit: Some(10f32),
                horizontal_search_distance: Some(5i32),
                movement_speed: Some(1.2f32),
                priority: Some(BehaviorMoveAroundTargetPriority {}),
                vertical_search_distance: Some(5i32),
            },
            behavior_nearest_attackable_target:
                super::super::components::BehaviorNearestAttackableTarget {
                    attack_interval: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            ("max".to_string(), crate::types::BedrockValue::Integer(0i64)),
                            ("min".to_string(), crate::types::BedrockValue::Integer(0i64)),
                        ]),
                    )),
                    attack_interval_min: None,
                    attack_owner: Some(false),
                    control_flags: Some(BehaviorNearestAttackableTargetControlFlags {}),
                    entity_types: Some(vec![
                        BehaviorNearestAttackableTargetEntityTypes {
                            check_if_outnumbered: None,
                            cooldown: None,
                            filters: Some(crate::types::BedrockValue::Object(
                                std::collections::HashMap::from([
                                    (
                                        "subject".to_string(),
                                        crate::types::BedrockValue::String("other".to_string()),
                                    ),
                                    (
                                        "test".to_string(),
                                        crate::types::BedrockValue::String("is_family".to_string()),
                                    ),
                                    (
                                        "value".to_string(),
                                        crate::types::BedrockValue::String("player".to_string()),
                                    ),
                                ]),
                            )),
                            max_dist: Some(24f32),
                            max_flee: None,
                            max_height: None,
                            must_see: None,
                            must_see_forget_duration: None,
                            priority: None,
                            reevaluate_description: None,
                            sprint_speed_multiplier: None,
                            walk_speed_multiplier: None,
                            within_default: None,
                        },
                        BehaviorNearestAttackableTargetEntityTypes {
                            check_if_outnumbered: None,
                            cooldown: None,
                            filters: Some(crate::types::BedrockValue::Object(
                                std::collections::HashMap::from([
                                    (
                                        "subject".to_string(),
                                        crate::types::BedrockValue::String("other".to_string()),
                                    ),
                                    (
                                        "test".to_string(),
                                        crate::types::BedrockValue::String("is_family".to_string()),
                                    ),
                                    (
                                        "value".to_string(),
                                        crate::types::BedrockValue::String("irongolem".to_string()),
                                    ),
                                ]),
                            )),
                            max_dist: Some(24f32),
                            max_flee: None,
                            max_height: None,
                            must_see: None,
                            must_see_forget_duration: None,
                            priority: None,
                            reevaluate_description: None,
                            sprint_speed_multiplier: None,
                            walk_speed_multiplier: None,
                            within_default: None,
                        },
                    ]),
                    must_reach: Some(false),
                    must_see: Some(true),
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
                    within_radius: Some(24f32),
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
                height: Some(1.77f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![
                    DamageSensorTriggers {
                        cause: Some("fall".to_string()),
                        damage_modifier: None,
                        damage_multiplier: None,
                        deals_damage: Some("false".to_string()),
                        on_damage: None,
                        on_damage_sound_event: None,
                    },
                    DamageSensorTriggers {
                        cause: Some("projectile".to_string()),
                        damage_modifier: None,
                        damage_multiplier: None,
                        deals_damage: Some("false".to_string()),
                        on_damage: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([(
                                "filters".to_string(),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("!=".to_string()),
                                        ),
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String(
                                                "damager".to_string(),
                                            ),
                                        ),
                                        (
                                            "test".to_string(),
                                            crate::types::BedrockValue::String(
                                                "is_family".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String(
                                                "wind_charge".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                            )]),
                        )),
                        on_damage_sound_event: None,
                    },
                ]),
            },
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Array(vec![
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:stop_playing_idle_ground_sound".to_string(),
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
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "on_ground".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(true),
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
                                                        "has_target".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Bool(true),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "domain".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:is_playing_idle_ground_sound"
                                                            .to_string(),
                                                    ),
                                                ),
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "==".to_string(),
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
                                    ]),
                                ),
                            ])),
                        ),
                    ])),
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:start_playing_idle_ground_sound".to_string(),
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
                                                        "minecraft:is_playing_idle_ground_sound"
                                                            .to_string(),
                                                    ),
                                                ),
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "!=".to_string(),
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
                                            std::collections::HashMap::from([(
                                                "any_of".to_string(),
                                                crate::types::BedrockValue::Array(vec![
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
                                                                    "on_ground".to_string(),
                                                                ),
                                                            ),
                                                            (
                                                                "value".to_string(),
                                                                crate::types::BedrockValue::Bool(
                                                                    false,
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
                                                                    "has_target".to_string(),
                                                                ),
                                                            ),
                                                            (
                                                                "value".to_string(),
                                                                crate::types::BedrockValue::Bool(
                                                                    false,
                                                                ),
                                                            ),
                                                        ]),
                                                    ),
                                                ]),
                                            )]),
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
                    "query.last_hit_by_player ? 10 : 0".to_string(),
                )),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(32f32),
            },
            health: super::super::components::Health {
                max: Some(30f32),
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
            knockback_resistance: super::super::components::KnockbackResistance {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/breeze.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.4f32),
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
                can_path_over_water: Some(false),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            reflect_projectiles: super::super::components::ReflectProjectiles {
                azimuth_angle: Some(crate::types::MolangOr::Expr(
                    "180.0 + Math.random(-20.0, 20.0)".to_string(),
                )),
                elevation_angle: Some(crate::types::MolangOr::Expr("0".to_string())),
                reflected_projectiles: Some(vec![
                    "xp_bottle".to_string(),
                    "thrown_trident".to_string(),
                    "shulker_bullet".to_string(),
                    "dragon_fireball".to_string(),
                    "arrow".to_string(),
                    "snowball".to_string(),
                    "egg".to_string(),
                    "fireball".to_string(),
                    "splash_potion".to_string(),
                    "ender_pearl".to_string(),
                    "wither_skull".to_string(),
                    "wither_skull_dangerous".to_string(),
                    "small_fireball".to_string(),
                    "lingering_potion".to_string(),
                    "llama_spit".to_string(),
                    "fireworks_rocket".to_string(),
                    "fishing_hook".to_string(),
                ]),
                reflection_scale: Some(crate::types::MolangOr::Expr("0.5".to_string())),
                reflection_sound: Some("reflect".to_string()),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "breeze".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreezeEvent {
    StartPlayingIdleGroundSound,
    StopPlayingIdleGroundSound,
}
