//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:hoglin`
pub struct Hoglin;
impl Hoglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:hoglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:hoglin`
#[derive(Bundle, Clone)]
pub struct HoglinBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_avoid_block: super::super::components::BehaviorAvoidBlock,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub leashable: super::super::components::Leashable,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:hoglin` entity with default Bedrock components
pub fn spawn_hoglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(HoglinBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_avoid_block: super::super::components::BehaviorAvoidBlock {
                avoid_block_sound: Some("retreat".to_string()),
                control_flags: Some(BehaviorAvoidBlockControlFlags {}),
                on_escape: Some(vec![]),
                priority: Some(BehaviorAvoidBlockPriority {}),
                search_height: Some(4i32),
                search_range: Some(8i32),
                sound_interval: Some(crate::types::RangeOrVal::Range {
                    min: 2f32,
                    max: 5f32,
                }),
                sprint_speed_modifier: Some(1f32),
                target_blocks: Some(vec![
                    crate::types::BedrockValue::String("minecraft:warped_fungus".to_string()),
                    crate::types::BedrockValue::String("minecraft:portal".to_string()),
                    crate::types::BedrockValue::String("minecraft:respawn_anchor".to_string()),
                ]),
                target_selection_method: Some("nearest".to_string()),
                tick_interval: Some(5i32),
                walk_speed_modifier: Some(1f32),
            },
            behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType {
                avoid_mob_sound: Some("retreat".to_string()),
                avoid_target_xz: Some(16i32),
                avoid_target_y: Some(7i32),
                control_flags: Some(BehaviorAvoidMobTypeControlFlags {}),
                entity_types: Some(vec![BehaviorAvoidMobTypeEntityTypes {
                    check_if_outnumbered: Some(true),
                    cooldown: None,
                    filters: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([(
                            "all_of".to_string(),
                            crate::types::BedrockValue::Array(vec![
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "subject".to_string(),
                                            crate::types::BedrockValue::String("other".to_string()),
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
                                                "piglin".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                            ]),
                        )]),
                    )),
                    max_dist: Some(10f32),
                    max_flee: None,
                    max_height: None,
                    must_see: None,
                    must_see_forget_duration: None,
                    priority: None,
                    reevaluate_description: None,
                    sprint_speed_multiplier: Some(1.2f32),
                    walk_speed_multiplier: None,
                    within_default: None,
                }]),
                ignore_visibility: Some(false),
                ignore_visibilty: None,
                max_dist: Some(3f32),
                max_flee: Some(10f32),
                on_escape_event: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String("".to_string()),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                ("AND".to_string(), crate::types::BedrockValue::Null),
                                ("NOT".to_string(), crate::types::BedrockValue::Null),
                                ("OR".to_string(), crate::types::BedrockValue::Null),
                                ("all".to_string(), crate::types::BedrockValue::Null),
                                ("all_of".to_string(), crate::types::BedrockValue::Null),
                                ("any".to_string(), crate::types::BedrockValue::Null),
                                ("any_of".to_string(), crate::types::BedrockValue::Null),
                                ("none_of".to_string(), crate::types::BedrockValue::Null),
                            ])),
                        ),
                        (
                            "target".to_string(),
                            crate::types::BedrockValue::String("self".to_string()),
                        ),
                    ]),
                )),
                priority: Some(BehaviorAvoidMobTypePriority {}),
                probability_per_strength: Some(1f32),
                remove_target: Some(true),
                sound_interval: Some(crate::types::RangeOrVal::Range {
                    min: 2f32,
                    max: 5f32,
                }),
                sprint_distance: Some(7f32),
                sprint_speed_multiplier: Some(1f32),
                walk_speed_multiplier: Some(1f32),
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
                    entity_types: Some(vec![BehaviorNearestAttackableTargetEntityTypes {
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
                                                    "player".to_string(),
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
                                                    "self".to_string(),
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
                                                    "minecraft:attack_cooldown".to_string(),
                                                ),
                                            ),
                                        ]),
                                    ),
                                ]),
                            )]),
                        )),
                        max_dist: Some(16f32),
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
                    within_radius: Some(16f32),
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
                height: Some(1.8f32),
                width: Some(0.6f32),
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Expr("Math.Random(1,7)".to_string())),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5 : 0".to_string(),
                )),
            },
            health: super::super::components::Health {
                max: Some(40f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(40f32),
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
                value: crate::types::RangeOrVal::Fixed(0.6f32),
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
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoglinComponentGroup {
    AngryHoglin,
    AttackCooldown,
    BecomeZombie,
    HuntableAdult,
    HoglinAdult,
    HoglinBaby,
    StartZombification,
    UnhuntableAdult,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoglinEvent {
    AttackCooldownCompleteEvent,
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    EscapedEvent,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    SpawnAdultUnhuntable,
    SpawnBaby,
    StartZombificationEvent,
    StopZombificationEvent,
}
