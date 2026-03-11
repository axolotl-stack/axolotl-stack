//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:fox`
pub struct Fox;
impl Fox {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fox";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:fox`
#[derive(Bundle, Clone)]
pub struct FoxBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_eat_carried_item: super::super::components::BehaviorEatCarriedItem,
    pub behavior_equip_item: super::super::components::BehaviorEquipItem,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_pickup_items: super::super::components::BehaviorPickupItems,
    pub behavior_raid_garden: super::super::components::BehaviorRaidGarden,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_look_around_and_sit:
        super::super::components::BehaviorRandomLookAroundAndSit,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_stalk_and_pounce_on_target:
        super::super::components::BehaviorStalkAndPounceOnTarget,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub block_climber: super::super::components::BlockClimber,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub equip_item: super::super::components::EquipItem,
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
    pub scheduler: super::super::components::Scheduler,
    pub shareables: super::super::components::Shareables,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:fox` entity with default Bedrock components
pub fn spawn_fox(commands: &mut Commands) -> Entity {
    commands
        .spawn(FoxBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: Some(0.6f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
                                    std::collections::HashMap::from([(
                                        "filter".to_string(),
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
                                                            "other".to_string(),
                                                        ),
                                                    ),
                                                    (
                                                        "test".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "trusts".to_string(),
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
                                                            "is_sneaking".to_string(),
                                                        ),
                                                    ),
                                                    (
                                                        "value".to_string(),
                                                        crate::types::BedrockValue::Bool(true),
                                                    ),
                                                ]),
                                            ),
                                        ]),
                                    )]),
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
                                                "polarbear".to_string(),
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
                                            crate::types::BedrockValue::String("wolf".to_string()),
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
                    sprint_speed_multiplier: Some(1.5f32),
                    walk_speed_multiplier: Some(1f32),
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
            behavior_eat_carried_item: super::super::components::BehaviorEatCarriedItem {
                delay_before_eating: Some(28f32),
                priority: Some(BehaviorEatCarriedItemPriority {}),
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
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(6f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
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
            behavior_raid_garden: super::super::components::BehaviorRaidGarden {
                blocks: Some(vec![
                    crate::types::BedrockValue::String("minecraft:sweet_berry_bush".to_string()),
                    crate::types::BedrockValue::String(
                        "minecraft:cave_vines_head_with_berries".to_string(),
                    ),
                    crate::types::BedrockValue::String(
                        "minecraft:cave_vines_body_with_berries".to_string(),
                    ),
                ]),
                eat_delay: Some(2i32),
                full_delay: Some(100i32),
                goal_radius: Some(0.8f32),
                initial_eat_delay: Some(2i32),
                max_to_eat: Some(0i32),
                priority: Some(BehaviorRaidGardenPriority {}),
                search_height: Some(2i32),
                search_range: Some(12i32),
                speed_multiplier: Some(BehaviorRaidGardenSpeedMultiplier {}),
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
                    continue_if_leashed: Some(false),
                    continue_sitting_on_reload: Some(false),
                    max_angle_of_view_horizontal: Some(30f32),
                    max_look_count: Some(5i32),
                    max_look_time: Some(100i32),
                    min_angle_of_view_horizontal: Some(-30f32),
                    min_look_count: Some(2i32),
                    min_look_time: Some(80i32),
                    priority: Some(BehaviorRandomLookAroundAndSitPriority {}),
                    probability: Some(0.001f32),
                    random_look_around_cooldown: Some(0i32),
                },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stalk_and_pounce_on_target:
                super::super::components::BehaviorStalkAndPounceOnTarget {
                    interest_time: Some(2f32),
                    leap_dist: Some(0.8f32),
                    leap_distance: Some(0.8f32),
                    leap_height: Some(0.9f32),
                    max_stalk_dist: Some(12f32),
                    pounce_max_dist: Some(5f32),
                    priority: Some(BehaviorStalkAndPounceOnTargetPriority {}),
                    set_persistent: Some(false),
                    stalk_speed: Some(1.2f32),
                    strike_dist: Some(2f32),
                    stuck_blocks: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "operator".to_string(),
                                crate::types::BedrockValue::String("==".to_string()),
                            ),
                            (
                                "subject".to_string(),
                                crate::types::BedrockValue::String("block".to_string()),
                            ),
                            (
                                "test".to_string(),
                                crate::types::BedrockValue::String("is_block".to_string()),
                            ),
                            (
                                "value".to_string(),
                                crate::types::BedrockValue::String("snow_layer".to_string()),
                            ),
                        ]),
                    )),
                    stuck_time: Some(2f32),
                },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(true),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(vec![
                    crate::types::BedrockValue::String("sweet_berries".to_string()),
                    crate::types::BedrockValue::String("glow_berries".to_string()),
                ]),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(16f32),
            },
            block_climber: super::super::components::BlockClimber,
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
                height: Some(0.7f32),
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
                    deals_damage: Some("no".to_string()),
                    on_damage: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([(
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "subject".to_string(),
                                    crate::types::BedrockValue::String("block".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_block".to_string()),
                                ),
                                (
                                    "value".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:sweet_berry_bush".to_string(),
                                    ),
                                ),
                            ])),
                        )]),
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
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Array(vec![
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:fox_configure_night".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_daytime".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(false)),
                            ])),
                        ),
                    ])),
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:fox_configure_day".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_daytime".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                            ])),
                        ),
                    ])),
                ])),
            },
            equip_item: super::super::components::EquipItem {
                can_wear_armor: Some(false),
                excluded_items: None,
            },
            health: super::super::components::Health {
                max: Some(10f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(10f32),
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
                value: crate::types::RangeOrVal::Fixed(0.3f32),
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
            scheduler: super::super::components::Scheduler {
                max_delay_secs: Some(0f32),
                min_delay_secs: Some(0f32),
                scheduled_events: Some(vec![
                    SchedulerScheduledEvents {
                        event: Some(crate::types::BedrockValue::String(
                            "minecraft:ambient_sleep".to_string(),
                        )),
                        filters: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("is_sleeping".to_string()),
                                ),
                                ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                            ]),
                        )),
                    },
                    SchedulerScheduledEvents {
                        event: Some(crate::types::BedrockValue::String(
                            "minecraft:ambient_night".to_string(),
                        )),
                        filters: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([(
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(vec![
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([
                                            (
                                                "test".to_string(),
                                                crate::types::BedrockValue::String(
                                                    "is_daytime".to_string(),
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
                                                crate::types::BedrockValue::String(">".to_string()),
                                            ),
                                            (
                                                "test".to_string(),
                                                crate::types::BedrockValue::String(
                                                    "distance_to_nearest_player".to_string(),
                                                ),
                                            ),
                                            (
                                                "value".to_string(),
                                                crate::types::BedrockValue::Integer(16i64),
                                            ),
                                        ]),
                                    ),
                                ]),
                            )]),
                        )),
                    },
                    SchedulerScheduledEvents {
                        event: Some(crate::types::BedrockValue::String(
                            "minecraft:ambient_normal".to_string(),
                        )),
                        filters: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([(
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(vec![
                                    crate::types::BedrockValue::Object(
                                        std::collections::HashMap::from([
                                            (
                                                "test".to_string(),
                                                crate::types::BedrockValue::String(
                                                    "is_sleeping".to_string(),
                                                ),
                                            ),
                                            (
                                                "value".to_string(),
                                                crate::types::BedrockValue::Bool(false),
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
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "is_daytime".to_string(),
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
                                                            "operator".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "<=".to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "test".to_string(),
                                                            crate::types::BedrockValue::String(
                                                                "distance_to_nearest_player"
                                                                    .to_string(),
                                                            ),
                                                        ),
                                                        (
                                                            "value".to_string(),
                                                            crate::types::BedrockValue::Integer(
                                                                16i64,
                                                            ),
                                                        ),
                                                    ]),
                                                ),
                                            ]),
                                        )]),
                                    ),
                                ]),
                            )]),
                        )),
                    },
                ]),
            },
            shareables: super::super::components::Shareables {
                all_items: Some(true),
                all_items_max_amount: Some(1i32),
                all_items_surplus_amount: Some(-1i32),
                all_items_want_amount: Some(-1i32),
                items: Some(vec![
                    ShareablesItems {
                        admire: None,
                        barter: None,
                        consume_item: None,
                        craft_into: None,
                        item: Some("minecraft:is_food".to_string()),
                        item_aux: None,
                        max_amount: Some(1i32),
                        pickup_limit: None,
                        pickup_only: None,
                        priority: Some(0i32),
                        stored_in_inventory: None,
                        surplus_amount: None,
                        want_amount: None,
                    },
                    ShareablesItems {
                        admire: None,
                        barter: None,
                        consume_item: None,
                        craft_into: None,
                        item: Some("minecraft:glow_berries".to_string()),
                        item_aux: None,
                        max_amount: Some(1i32),
                        pickup_limit: None,
                        pickup_only: None,
                        priority: Some(0i32),
                        stored_in_inventory: None,
                        surplus_amount: None,
                        want_amount: None,
                    },
                    ShareablesItems {
                        admire: None,
                        barter: None,
                        consume_item: None,
                        craft_into: None,
                        item: Some("minecraft:bundle".to_string()),
                        item_aux: None,
                        max_amount: Some(1i32),
                        pickup_limit: None,
                        pickup_only: None,
                        priority: Some(1i32),
                        stored_in_inventory: None,
                        surplus_amount: None,
                        want_amount: None,
                    },
                ]),
                singular_pickup: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "fox".to_string(),
                    "lightweight".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxComponentGroup {
    DefendingFox,
    DocileFox,
    FoxAdult,
    FoxAmbientDefendingTarget,
    FoxAmbientNight,
    FoxAmbientNormal,
    FoxAmbientSleep,
    FoxArctic,
    FoxBaby,
    FoxDay,
    FoxNight,
    FoxRed,
    FoxThunderstorm,
    FoxWithItem,
    TrustingFox,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxEvent {
    AgeableGrowUp,
    AmbientNight,
    AmbientNormal,
    AmbientSleep,
    EntityBorn,
    EntitySpawned,
    FoxConfigureDay,
    FoxConfigureDefending,
    FoxConfigureDocileDay,
    FoxConfigureDocileNight,
    FoxConfigureNight,
    FoxConfigureThunderstorm,
}
