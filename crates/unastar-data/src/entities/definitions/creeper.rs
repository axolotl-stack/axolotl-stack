//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:creeper`
pub struct Creeper;
impl Creeper {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:creeper";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:creeper`
#[derive(Bundle, Clone)]
pub struct CreeperBundle {
    pub attack: super::super::components::Attack,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_melee_attack: super::super::components::BehaviorMeleeAttack,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_swell: super::super::components::BehaviorSwell,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub on_target_escape: super::super::components::OnTargetEscape,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub target_nearby_sensor: super::super::components::TargetNearbySensor,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:creeper` entity with default Bedrock components
pub fn spawn_creeper(commands: &mut Commands) -> Entity {
    commands
        .spawn(CreeperBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType {
                avoid_mob_sound: Some("undefined".to_string()),
                avoid_target_xz: Some(16i32),
                avoid_target_y: Some(7i32),
                control_flags: Some(BehaviorAvoidMobTypeControlFlags {}),
                entity_types: Some(vec![BehaviorAvoidMobTypeEntityTypes {
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
                                                "ocelot".to_string(),
                                            ),
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
                                            crate::types::BedrockValue::String("cat".to_string()),
                                        ),
                                    ]),
                                ),
                            ]),
                        )]),
                    )),
                    max_dist: Some(6f32),
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
                remove_target: Some(false),
                sound_interval: Some(crate::types::RangeOrVal::Range {
                    min: 3f32,
                    max: 8f32,
                }),
                sprint_distance: Some(7f32),
                sprint_speed_multiplier: Some(1f32),
                walk_speed_multiplier: Some(1f32),
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
                target_distance: None,
            },
            behavior_melee_attack: super::super::components::BehaviorMeleeAttack {
                attack_once: Some(false),
                attack_types: None,
                can_spread_on_fire: Some(false),
                control_flags: None,
                cooldown_time: Some(1f32),
                inner_boundary_time_increase: Some(0.25f32),
                max_dist: None,
                max_path_time: Some(0.55f32),
                melee_fov: Some(90f32),
                min_path_time: Some(0.2f32),
                on_attack: None,
                on_kill: None,
                outer_boundary_time_increase: Some(0.5f32),
                path_fail_time_increase: Some(0.75f32),
                path_inner_boundary: Some(16f32),
                path_outer_boundary: Some(32f32),
                priority: Some(BehaviorMeleeAttackPriority {}),
                random_stop_interval: Some(0i32),
                reach_multiplier: Some(0f32),
                require_complete_path: Some(false),
                set_persistent: None,
                speed_multiplier: Some(BehaviorMeleeAttackSpeedMultiplier {}),
                target_dist: None,
                track_target: Some(false),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
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
            behavior_swell: super::super::components::BehaviorSwell {
                priority: Some(BehaviorSwellPriority {}),
                start_distance: Some(2.5f32),
                stop_distance: Some(6f32),
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
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: None,
                    damage_modifier: None,
                    damage_multiplier: None,
                    deals_damage: Some("false".to_string()),
                    on_damage: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:become_charged".to_string(),
                                ),
                            ),
                            (
                                "filters".to_string(),
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
                                                "lightning".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                            ),
                        ]),
                    )),
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5 : 0".to_string(),
                )),
            },
            health: super::super::components::Health {
                max: Some(20f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            interact: super::super::components::Interact {
                interactions: Some(vec![InteractInteractions {
                    add_items: None,
                    admire: None,
                    barter: None,
                    cooldown: None,
                    cooldown_after_being_attacked: None,
                    drop_item_slot: None,
                    drop_item_y_offset: None,
                    equip_item_slot: None,
                    give_item: None,
                    health_amount: None,
                    hurt_item: Some(1i32),
                    interact_text: Some("action.interact.creeper".to_string()),
                    on_interact: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:start_exploding_forced".to_string(),
                                ),
                            ),
                            (
                                "filters".to_string(),
                                crate::types::BedrockValue::Object(
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
                                                        "domain".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "hand".to_string(),
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
                                                            "has_equipment".to_string(),
                                                        ),
                                                    ),
                                                    (
                                                        "value".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "flint_and_steel".to_string(),
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
                                                        "test".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "has_component".to_string(),
                                                        ),
                                                    ),
                                                    (
                                                        "value".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "minecraft:explode".to_string(),
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ]),
                                    )]),
                                ),
                            ),
                            (
                                "target".to_string(),
                                crate::types::BedrockValue::String("self".to_string()),
                            ),
                        ]),
                    )),
                    particle_on_start: None,
                    play_sounds: Some("ignite".to_string()),
                    repair_entity_item: None,
                    spawn_entities: None,
                    spawn_items: None,
                    swing: Some(true),
                    take_item: None,
                    transform_to_item: None,
                    use_item: None,
                    vibration: None,
                }]),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/creeper.json".to_string(),
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
            on_target_escape: super::super::components::OnTargetEscape {
                value: crate::types::BedrockValue::Null,
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
            target_nearby_sensor: super::super::components::TargetNearbySensor {
                inside_range: Some(2.5f32),
                must_see: Some(true),
                on_inside_range: Some(TargetNearbySensorOnInsideRange {
                    event: Some("minecraft:start_exploding".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                on_outside_range: Some(TargetNearbySensorOnOutsideRange {
                    event: Some("minecraft:stop_exploding".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                on_vision_lost_inside_range: Some(TargetNearbySensorOnVisionLostInsideRange {
                    event: Some("minecraft:stop_exploding".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                outside_range: Some(6f32),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "creeper".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreeperComponentGroup {
    ChargedCreeper,
    ChargedExploding,
    Exploding,
    ForcedChargedExploding,
    ForcedExploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreeperEvent {
    BecomeCharged,
    StartExploding,
    StartExplodingForced,
    StopExploding,
}
