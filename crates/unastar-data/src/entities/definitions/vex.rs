//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:vex`
pub struct Vex;
impl Vex {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vex";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vex`
#[derive(Bundle, Clone)]
pub struct VexBundle {
    pub attack: super::super::components::Attack,
    pub behavior_charge_attack: super::super::components::BehaviorChargeAttack,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_entity: super::super::components::BehaviorLookAtEntity,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub equipment: super::super::components::Equipment,
    pub experience_reward: super::super::components::ExperienceReward,
    pub fire_immune: super::super::components::FireImmune,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub health: super::super::components::Health,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:vex` entity with default Bedrock components
pub fn spawn_vex(commands: &mut Commands) -> Entity {
    commands
        .spawn(VexBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_charge_attack: super::super::components::BehaviorChargeAttack {
                max_distance: Some(3f32),
                min_distance: Some(2f32),
                priority: Some(BehaviorChargeAttackPriority {}),
                speed_multiplier: Some(BehaviorChargeAttackSpeedMultiplier {}),
                success_rate: Some(0.1428f32),
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
            behavior_look_at_entity: super::super::components::BehaviorLookAtEntity {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
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
                            crate::types::BedrockValue::String("mob".to_string()),
                        ),
                    ]),
                )),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtEntityPriority {}),
                probability: Some(0.02f32),
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
                                std::collections::HashMap::from([(
                                    "any_of".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
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
                                                        "player".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
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
                                                        "irongolem".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
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
                                                        "wandering_trader".to_string(),
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
                                                        "villager".to_string(),
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
                                                        "has_component".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:is_baby".to_string(),
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
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(0.8f32),
                width: Some(0.4f32),
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
            equipment: super::super::components::Equipment {
                slot_drop_chance: None,
                table: Some("loot_tables/entities/vex_gear.json".to_string()),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5 + (query.equipment_count * Math.Random(1,3)) : 0"
                        .to_string(),
                )),
            },
            fire_immune: super::super::components::FireImmune,
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(false),
                emit_move: Some(false),
                emit_swim: Some(false),
            },
            health: super::super::components::Health {
                max: Some(14f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(14f32),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1f32),
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
                can_path_over_water: Some(true),
                can_sink: Some(true),
                can_swim: Some(false),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(false),
            },
            physics: super::super::components::Physics {
                has_collision: Some(false),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["vex".to_string(), "monster".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VexComponentGroup {
    PeriodicDamage,
    StartDamageTimer,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VexEvent {
    AddDamageTimer,
    AddPeriodicDamage,
}
