//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:iron_golem`
pub struct IronGolem;
impl IronGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:iron_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:iron_golem`
#[derive(Bundle, Clone)]
pub struct IronGolemBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack,
    pub behavior_move_through_village: super::super::components::BehaviorMoveThroughVillage,
    pub behavior_move_towards_dwelling_restriction:
        super::super::components::BehaviorMoveTowardsDwellingRestriction,
    pub behavior_move_towards_target: super::super::components::BehaviorMoveTowardsTarget,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_offer_flower: super::super::components::BehaviorOfferFlower,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_target_when_pushed: super::super::components::BehaviorTargetWhenPushed,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub preferred_path: super::super::components::PreferredPath,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:iron_golem` entity with default Bedrock components
pub fn spawn_iron_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(IronGolemBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Range {
                    min: 7f32,
                    max: 21f32,
                },
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: Some(2f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: Some(vec![BehaviorHurtByTargetEntityTypes {
                    check_if_outnumbered: None,
                    cooldown: None,
                    filters: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "operator".to_string(),
                                crate::types::BedrockValue::String("not".to_string()),
                            ),
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
                                crate::types::BedrockValue::String("creeper".to_string()),
                            ),
                        ]),
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
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack {
                attack_once: Some(false),
                attack_types: None,
                can_spread_on_fire: Some(false),
                control_flags: None,
                cooldown_time: Some(1f32),
                horizontal_reach: Some(0.8f32),
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
                priority: Some(BehaviorMeleeBoxAttackPriority {}),
                random_stop_interval: Some(0i32),
                reach_multiplier: None,
                require_complete_path: Some(false),
                set_persistent: None,
                speed_multiplier: Some(BehaviorMeleeBoxAttackSpeedMultiplier {}),
                target_dist: None,
                track_target: Some(true),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            behavior_move_through_village: super::super::components::BehaviorMoveThroughVillage {
                only_at_night: Some(true),
                priority: Some(BehaviorMoveThroughVillagePriority {}),
                speed_multiplier: Some(BehaviorMoveThroughVillageSpeedMultiplier {}),
            },
            behavior_move_towards_dwelling_restriction:
                super::super::components::BehaviorMoveTowardsDwellingRestriction {
                    priority: Some(BehaviorMoveTowardsDwellingRestrictionPriority {}),
                    speed_multiplier: Some(
                        BehaviorMoveTowardsDwellingRestrictionSpeedMultiplier {},
                    ),
                },
            behavior_move_towards_target: super::super::components::BehaviorMoveTowardsTarget {
                priority: Some(BehaviorMoveTowardsTargetPriority {}),
                speed_multiplier: Some(BehaviorMoveTowardsTargetSpeedMultiplier {}),
                within_radius: Some(32f32),
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
                                                        "monster".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "not".to_string(),
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
                                                        "creeper".to_string(),
                                                    ),
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
                            within_default: Some(10f32),
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
                                                        "hoglin".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "not".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "is_difficulty".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "peaceful".to_string(),
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
                                                        "zoglin".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "not".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "is_difficulty".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "peaceful".to_string(),
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
                        },
                    ]),
                    must_reach: Some(true),
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
            behavior_offer_flower: super::super::components::BehaviorOfferFlower {
                chance_to_start: Some(0f32),
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "test".to_string(),
                        crate::types::BedrockValue::String("is_daytime".to_string()),
                    )]),
                )),
                max_head_rotation_y: Some(30f32),
                max_offer_flower_duration: Some(20f32),
                max_rotation_x: Some(30f32),
                priority: Some(BehaviorOfferFlowerPriority {}),
                search_area: Some(vec![6f32, 2f32, 6f32]),
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
                xz_dist: Some(16i32),
                y_dist: Some(7i32),
            },
            behavior_target_when_pushed: super::super::components::BehaviorTargetWhenPushed {
                entity_types: Some(vec![BehaviorTargetWhenPushedEntityTypes {
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
                                                "monster".to_string(),
                                            ),
                                        ),
                                    ]),
                                ),
                                crate::types::BedrockValue::Object(
                                    std::collections::HashMap::from([
                                        (
                                            "operator".to_string(),
                                            crate::types::BedrockValue::String("not".to_string()),
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
                                                "creeper".to_string(),
                                            ),
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
                percent_chance: Some(5f32),
                priority: Some(BehaviorTargetWhenPushedPriority {}),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(2.9f32),
                width: Some(1.4f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: Some("fall".to_string()),
                    damage_modifier: None,
                    damage_multiplier: None,
                    deals_damage: Some("no".to_string()),
                    on_damage: None,
                    on_damage_sound_event: None,
                }]),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(100f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(100f32),
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
                    health_amount: Some(25i32),
                    hurt_item: None,
                    interact_text: Some("action.interact.repair".to_string()),
                    on_interact: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([(
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
                                                        "iron_ingot".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([(
                                                "test".to_string(),
                                                crate::types::BedrockValue::String(
                                                    "is_missing_health".to_string(),
                                                ),
                                            )]),
                                        ),
                                    ]),
                                ),
                            ])),
                        )]),
                    )),
                    particle_on_start: None,
                    play_sounds: Some("irongolem.repair".to_string()),
                    repair_entity_item: None,
                    spawn_entities: None,
                    spawn_items: None,
                    swing: None,
                    take_item: None,
                    transform_to_item: None,
                    use_item: Some(true),
                    vibration: None,
                }]),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            knockback_resistance: super::super::components::KnockbackResistance {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1f32),
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
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/iron_golem.json".to_string(),
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
                is_amphibious: Some(false),
            },
            persistent: super::super::components::Persistent,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            preferred_path: super::super::components::PreferredPath {
                default_block_cost: Some(1.5f32),
                jump_cost: Some(5i32),
                max_fall_blocks: Some(1i32),
                preferred_path_blocks: Some(vec![
                    PreferredPathPreferredPathBlocks {
                        blocks: Some(vec![crate::types::BedrockValue::String(
                            "grass_path".to_string(),
                        )]),
                        cost: Some(0f32),
                    },
                    PreferredPathPreferredPathBlocks {
                        blocks: Some(vec![
                            crate::types::BedrockValue::String("cobblestone".to_string()),
                            crate::types::BedrockValue::String("stone".to_string()),
                            crate::types::BedrockValue::String("granite".to_string()),
                            crate::types::BedrockValue::String("polished_granite".to_string()),
                            crate::types::BedrockValue::String("diorite".to_string()),
                            crate::types::BedrockValue::String("polished_diorite".to_string()),
                            crate::types::BedrockValue::String("andesite".to_string()),
                            crate::types::BedrockValue::String("polished_andesite".to_string()),
                            crate::types::BedrockValue::String("stone_bricks".to_string()),
                            crate::types::BedrockValue::String("mossy_stone_bricks".to_string()),
                            crate::types::BedrockValue::String("cracked_stone_bricks".to_string()),
                            crate::types::BedrockValue::String("chiseled_stone_bricks".to_string()),
                            crate::types::BedrockValue::String("sandstone".to_string()),
                            crate::types::BedrockValue::String("cut_sandstone".to_string()),
                            crate::types::BedrockValue::String("chiseled_sandstone".to_string()),
                            crate::types::BedrockValue::String("smooth_sandstone".to_string()),
                            crate::types::BedrockValue::String("mossy_cobblestone".to_string()),
                            crate::types::BedrockValue::String("smooth_stone_slab".to_string()),
                            crate::types::BedrockValue::String("sandstone_slab".to_string()),
                            crate::types::BedrockValue::String("cobblestone_slab".to_string()),
                            crate::types::BedrockValue::String("brick_slab".to_string()),
                            crate::types::BedrockValue::String("stone_brick_slab".to_string()),
                            crate::types::BedrockValue::String("quartz_slab".to_string()),
                            crate::types::BedrockValue::String("nether_brick_slab".to_string()),
                            crate::types::BedrockValue::String("red_sandstone_slab".to_string()),
                            crate::types::BedrockValue::String("purpur_slab".to_string()),
                            crate::types::BedrockValue::String("prismarine_slab".to_string()),
                            crate::types::BedrockValue::String("dark_prismarine_slab".to_string()),
                            crate::types::BedrockValue::String("prismarine_brick_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "mossy_cobblestone_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("smooth_sandstone_slab".to_string()),
                            crate::types::BedrockValue::String("red_nether_brick_slab".to_string()),
                            crate::types::BedrockValue::String("end_stone_brick_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "smooth_red_sandstone_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "polished_andesite_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("andesite_slab".to_string()),
                            crate::types::BedrockValue::String("diorite_slab".to_string()),
                            crate::types::BedrockValue::String("polished_diorite_slab".to_string()),
                            crate::types::BedrockValue::String("granite_slab".to_string()),
                            crate::types::BedrockValue::String("polished_granite_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "mossy_stone_brick_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("smooth_quartz_slab".to_string()),
                            crate::types::BedrockValue::String("normal_stone_slab".to_string()),
                            crate::types::BedrockValue::String("cut_sandstone_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "cut_red_sandstone_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "smooth_stone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("sandstone_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "cobblestone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("brick_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "stone_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("quartz_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "nether_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "red_sandstone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("purpur_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "prismarine_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "dark_prismarine_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "prismarine_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "mossy_cobblestone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "smooth_sandstone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "red_nether_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "end_stone_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "smooth_red_sandstone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "polished_andesite_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("andesite_double_slab".to_string()),
                            crate::types::BedrockValue::String("diorite_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "polished_diorite_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("granite_double_slab".to_string()),
                            crate::types::BedrockValue::String(
                                "polished_granite_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "mossy_stone_brick_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "smooth_quartz_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "normal_stone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "cut_sandstone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "cut_red_sandstone_double_slab".to_string(),
                            ),
                            crate::types::BedrockValue::String("oak_slab".to_string()),
                            crate::types::BedrockValue::String("spruce_slab".to_string()),
                            crate::types::BedrockValue::String("birch_slab".to_string()),
                            crate::types::BedrockValue::String("jungle_slab".to_string()),
                            crate::types::BedrockValue::String("acacia_slab".to_string()),
                            crate::types::BedrockValue::String("dark_oak_slab".to_string()),
                            crate::types::BedrockValue::String("oak_double_slab".to_string()),
                            crate::types::BedrockValue::String("spruce_double_slab".to_string()),
                            crate::types::BedrockValue::String("birch_double_slab".to_string()),
                            crate::types::BedrockValue::String("jungle_double_slab".to_string()),
                            crate::types::BedrockValue::String("acacia_double_slab".to_string()),
                            crate::types::BedrockValue::String("dark_oak_double_slab".to_string()),
                            crate::types::BedrockValue::String("oak_planks".to_string()),
                            crate::types::BedrockValue::String("spruce_planks".to_string()),
                            crate::types::BedrockValue::String("birch_planks".to_string()),
                            crate::types::BedrockValue::String("jungle_planks".to_string()),
                            crate::types::BedrockValue::String("acacia_planks".to_string()),
                            crate::types::BedrockValue::String("dark_oak_planks".to_string()),
                            crate::types::BedrockValue::String("brick_block".to_string()),
                            crate::types::BedrockValue::String("nether_brick".to_string()),
                            crate::types::BedrockValue::String("red_nether_brick".to_string()),
                            crate::types::BedrockValue::String("end_bricks".to_string()),
                            crate::types::BedrockValue::String("red_sandstone".to_string()),
                            crate::types::BedrockValue::String("cut_red_sandstone".to_string()),
                            crate::types::BedrockValue::String(
                                "chiseled_red_sandstone".to_string(),
                            ),
                            crate::types::BedrockValue::String("smooth_red_sandstone".to_string()),
                            crate::types::BedrockValue::String("white_stained_glass".to_string()),
                            crate::types::BedrockValue::String("orange_stained_glass".to_string()),
                            crate::types::BedrockValue::String("magenta_stained_glass".to_string()),
                            crate::types::BedrockValue::String(
                                "light_blue_stained_glass".to_string(),
                            ),
                            crate::types::BedrockValue::String("yellow_stained_glass".to_string()),
                            crate::types::BedrockValue::String("lime_stained_glass".to_string()),
                            crate::types::BedrockValue::String("pink_stained_glass".to_string()),
                            crate::types::BedrockValue::String("gray_stained_glass".to_string()),
                            crate::types::BedrockValue::String(
                                "light_gray_stained_glass".to_string(),
                            ),
                            crate::types::BedrockValue::String("cyan_stained_glass".to_string()),
                            crate::types::BedrockValue::String("purple_stained_glass".to_string()),
                            crate::types::BedrockValue::String("blue_stained_glass".to_string()),
                            crate::types::BedrockValue::String("brown_stained_glass".to_string()),
                            crate::types::BedrockValue::String("green_stained_glass".to_string()),
                            crate::types::BedrockValue::String("red_stained_glass".to_string()),
                            crate::types::BedrockValue::String("black_stained_glass".to_string()),
                            crate::types::BedrockValue::String("glass".to_string()),
                            crate::types::BedrockValue::String("glowstone".to_string()),
                            crate::types::BedrockValue::String("prismarine".to_string()),
                            crate::types::BedrockValue::String("emerald_block".to_string()),
                            crate::types::BedrockValue::String("diamond_block".to_string()),
                            crate::types::BedrockValue::String("lapis_block".to_string()),
                            crate::types::BedrockValue::String("gold_block".to_string()),
                            crate::types::BedrockValue::String("redstone_block".to_string()),
                            crate::types::BedrockValue::String(
                                "purple_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "white_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "orange_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "magenta_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "light_blue_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "yellow_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "lime_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "pink_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "gray_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "silver_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "cyan_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "blue_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "brown_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String(
                                "green_glazed_terracotta".to_string(),
                            ),
                            crate::types::BedrockValue::String("red_glazed_terracotta".to_string()),
                            crate::types::BedrockValue::String(
                                "black_glazed_terracotta".to_string(),
                            ),
                        ]),
                        cost: Some(1f32),
                    },
                    PreferredPathPreferredPathBlocks {
                        blocks: Some(vec![
                            crate::types::BedrockValue::String("bed".to_string()),
                            crate::types::BedrockValue::String("lectern".to_string()),
                            crate::types::BedrockValue::String("composter".to_string()),
                            crate::types::BedrockValue::String("grindstone".to_string()),
                            crate::types::BedrockValue::String("blast_furnace".to_string()),
                            crate::types::BedrockValue::String("smoker".to_string()),
                            crate::types::BedrockValue::String("fletching_table".to_string()),
                            crate::types::BedrockValue::String("cartography_table".to_string()),
                            crate::types::BedrockValue::String("brewing_stand".to_string()),
                            crate::types::BedrockValue::String("smithing_table".to_string()),
                            crate::types::BedrockValue::String("cauldron".to_string()),
                            crate::types::BedrockValue::String("barrel".to_string()),
                            crate::types::BedrockValue::String("loom".to_string()),
                            crate::types::BedrockValue::String("stonecutter".to_string()),
                        ]),
                        cost: Some(50f32),
                    },
                ]),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["irongolem".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IronGolemComponentGroup {
    PlayerCreated,
    VillageCreated,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IronGolemEvent {
    FromPlayer,
    FromVillage,
}
