//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:evocation_illager`
pub struct EvocationIllager;
impl EvocationIllager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:evocation_illager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:evocation_illager`
#[derive(Bundle, Clone)]
pub struct EvocationIllagerBundle {
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_equip_item: super::super::components::BehaviorEquipItem,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_entity: super::super::components::BehaviorLookAtEntity,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_pickup_items: super::super::components::BehaviorPickupItems,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_send_event: super::super::components::BehaviorSendEvent,
    pub behavior_summon_entity: super::super::components::BehaviorSummonEntity,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub can_join_raid: super::super::components::CanJoinRaid,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub equip_item: super::super::components::EquipItem,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
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
    pub shareables: super::super::components::Shareables,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:evocation_illager` entity with default Bedrock components
pub fn spawn_evocation_illager(commands: &mut Commands) -> Entity {
    commands
        .spawn(EvocationIllagerBundle {
            behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType {
                avoid_mob_sound: Some("undefined".to_string()),
                avoid_target_xz: Some(16i32),
                avoid_target_y: Some(7i32),
                control_flags: Some(BehaviorAvoidMobTypeControlFlags {}),
                entity_types: Some(vec![
                    BehaviorAvoidMobTypeEntityTypes {
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
                        max_dist: Some(8f32),
                        max_flee: None,
                        max_height: None,
                        must_see: None,
                        must_see_forget_duration: None,
                        priority: None,
                        reevaluate_description: None,
                        sprint_speed_multiplier: Some(1f32),
                        walk_speed_multiplier: Some(0.6f32),
                        within_default: None,
                    },
                    BehaviorAvoidMobTypeEntityTypes {
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
                                    crate::types::BedrockValue::String("creaking".to_string()),
                                ),
                            ]),
                        )),
                        max_dist: Some(8f32),
                        max_flee: None,
                        max_height: None,
                        must_see: None,
                        must_see_forget_duration: None,
                        priority: None,
                        reevaluate_description: None,
                        sprint_speed_multiplier: Some(1.2f32),
                        walk_speed_multiplier: None,
                        within_default: None,
                    },
                ]),
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
            behavior_equip_item: super::super::components::BehaviorEquipItem {
                priority: Some(BehaviorEquipItemPriority {}),
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
                                crate::types::BedrockValue::String("is_family".to_string()),
                            ),
                            (
                                "value".to_string(),
                                crate::types::BedrockValue::String("illager".to_string()),
                            ),
                        ]),
                    )),
                    max_dist: Some(64f32),
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
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtEntityPriority {}),
                probability: Some(0.02f32),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(3f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(1f32),
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
                                                        "snowgolem".to_string(),
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
                            max_dist: Some(20f32),
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
                            max_dist: Some(20f32),
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
            behavior_pickup_items: super::super::components::BehaviorPickupItems {
                can_pickup_any_item: Some(false),
                can_pickup_to_hand_or_equipment: Some(true),
                cooldown_after_being_attacked: None,
                excluded_items: None,
                goal_radius: Some(2f32),
                max_dist: Some(3f32),
                pickup_based_on_chance: Some(false),
                pickup_same_items_as_in_hand: None,
                priority: Some(BehaviorPickupItemsPriority {}),
                search_height: None,
                speed_multiplier: Some(BehaviorPickupItemsSpeedMultiplier {}),
                track_target: Some(false),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_send_event: super::super::components::BehaviorSendEvent {
                cast_duration: None,
                event_choices: Some(vec![BehaviorSendEventEventChoices {
                    cast_duration: Some(3f32),
                    cooldown_time: Some(5f32),
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
                                            crate::types::BedrockValue::String("sheep".to_string()),
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
                                                "is_color".to_string(),
                                            ),
                                        ),
                                        (
                                            "value".to_string(),
                                            crate::types::BedrockValue::String("blue".to_string()),
                                        ),
                                    ]),
                                ),
                            ]),
                        )]),
                    )),
                    max_activation_range: Some(16f32),
                    min_activation_range: Some(0f32),
                    particle_color: Some("#FFB38033".to_string()),
                    sequence: None,
                    start_sound_event: Some("cast.spell".to_string()),
                    weight: Some(3i32),
                }]),
                look_at_target: Some(true),
                priority: Some(BehaviorSendEventPriority {}),
                sequence: None,
            },
            behavior_summon_entity: super::super::components::BehaviorSummonEntity {
                priority: Some(BehaviorSummonEntityPriority {}),
                summon_choices: Some(vec![
                    BehaviorSummonEntitySummonChoices {
                        cast_duration: Some(2f32),
                        cooldown_time: Some(5f32),
                        do_casting: None,
                        filters: None,
                        max_activation_range: Some(3f32),
                        min_activation_range: Some(0f32),
                        particle_color: Some(crate::types::MolangOr::Expr("#FF664D59".to_string())),
                        sequence: None,
                        start_sound_event: Some("cast.spell".to_string()),
                        weight: Some(3f32),
                    },
                    BehaviorSummonEntitySummonChoices {
                        cast_duration: Some(2f32),
                        cooldown_time: Some(5f32),
                        do_casting: None,
                        filters: None,
                        max_activation_range: None,
                        min_activation_range: Some(3f32),
                        particle_color: Some(crate::types::MolangOr::Expr("#FF664D59".to_string())),
                        sequence: None,
                        start_sound_event: Some("cast.spell".to_string()),
                        weight: Some(3f32),
                    },
                    BehaviorSummonEntitySummonChoices {
                        cast_duration: Some(5f32),
                        cooldown_time: Some(17f32),
                        do_casting: None,
                        filters: None,
                        max_activation_range: None,
                        min_activation_range: None,
                        particle_color: Some(crate::types::MolangOr::Expr("#FFB3B3CC".to_string())),
                        sequence: None,
                        start_sound_event: None,
                        weight: Some(1f32),
                    },
                ]),
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
            can_join_raid: super::super::components::CanJoinRaid,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            equip_item: super::super::components::EquipItem {
                can_wear_armor: None,
                excluded_items: None,
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr("10".to_string())),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(24f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(24f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/evocation_illager.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.5f32),
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
                can_open_doors: Some(true),
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
            shareables: super::super::components::Shareables {
                all_items: Some(false),
                all_items_max_amount: Some(-1i32),
                all_items_surplus_amount: Some(-1i32),
                all_items_want_amount: Some(-1i32),
                items: Some(vec![ShareablesItems {
                    admire: None,
                    barter: None,
                    consume_item: None,
                    craft_into: None,
                    item: Some("minecraft:banner:15".to_string()),
                    item_aux: None,
                    max_amount: None,
                    pickup_limit: None,
                    pickup_only: None,
                    priority: Some(0i32),
                    stored_in_inventory: None,
                    surplus_amount: Some(1i32),
                    want_amount: Some(1i32),
                }]),
                singular_pickup: Some(false),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "evocation_illager".to_string(),
                    "monster".to_string(),
                    "illager".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvocationIllagerComponentGroup {
    Celebrate,
    RaidConfiguration,
    RaidDespawn,
    RaidPersistence,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvocationIllagerEvent {
    RaidExpired,
    SpawnForRaid,
    StartCelebrating,
    StopCelebrating,
}
