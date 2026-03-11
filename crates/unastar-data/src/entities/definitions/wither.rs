//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:wither`
pub struct Wither;
impl Wither {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wither";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wither`
#[derive(Bundle, Clone)]
pub struct WitherBundle {
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_look_at_target: super::super::components::BehaviorLookAtTarget,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_wither_random_attack_pos_goal:
        super::super::components::BehaviorWitherRandomAttackPosGoal,
    pub behavior_wither_target_highest_damage:
        super::super::components::BehaviorWitherTargetHighestDamage,
    pub boss: super::super::components::Boss,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub experience_reward: super::super::components::ExperienceReward,
    pub fire_immune: super::super::components::FireImmune,
    pub health: super::super::components::Health,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:wither` entity with default Bedrock components
pub fn spawn_wither(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitherBundle {
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
                target_distance: None,
            },
            behavior_look_at_target: super::super::components::BehaviorLookAtTarget {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtTargetPriority {}),
                probability: Some(0.02f32),
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
                            max_dist: Some(70f32),
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
                                std::collections::HashMap::from([(
                                    "all_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "!=".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "other".to_string(),
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
                                                        "undead".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "!=".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "other".to_string(),
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
                                                        "inanimate".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                )]),
                            )),
                            max_dist: Some(70f32),
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
                    within_radius: Some(0f32),
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
            behavior_wither_random_attack_pos_goal:
                super::super::components::BehaviorWitherRandomAttackPosGoal {
                    priority: Some(BehaviorWitherRandomAttackPosGoalPriority {}),
                },
            behavior_wither_target_highest_damage:
                super::super::components::BehaviorWitherTargetHighestDamage {
                    entity_types: None,
                    priority: Some(BehaviorWitherTargetHighestDamagePriority {}),
                },
            boss: super::super::components::Boss {
                hud_range: Some(55i32),
                name: Some("55".to_string()),
                should_darken_sky: Some(true),
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
            can_fly: super::super::components::CanFly {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(3f32),
                width: Some(1f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: None,
                    damage_modifier: None,
                    damage_multiplier: None,
                    deals_damage: Some("no".to_string()),
                    on_damage: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([(
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
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
                                    crate::types::BedrockValue::String("undead".to_string()),
                                ),
                            ])),
                        )]),
                    )),
                    on_damage_sound_event: None,
                }]),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr("50".to_string())),
            },
            fire_immune: super::super::components::FireImmune,
            health: super::super::components::Health {
                max: Some(600f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(600f32),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/wither_boss.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(180f32),
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
                can_path_over_water: Some(true),
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "wither".to_string(),
                    "skeleton".to_string(),
                    "monster".to_string(),
                    "undead".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitherEvent {
    EntitySpawned,
}
