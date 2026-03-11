//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:wolf`
pub struct Wolf;
impl Wolf {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wolf";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wolf`
#[derive(Bundle, Clone)]
pub struct WolfBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_beg: super::super::components::BehaviorBeg,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_leap_at_target: super::super::components::BehaviorLeapAtTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_stay_while_sitting: super::super::components::BehaviorStayWhileSitting,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub environment_sensor: super::super::components::EnvironmentSensor,
    pub healable: super::super::components::Healable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
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
/// Spawn a new `minecraft:wolf` entity with default Bedrock components
pub fn spawn_wolf(commands: &mut Commands) -> Entity {
    commands
        .spawn(WolfBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: Some(0.8f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_beg: super::super::components::BehaviorBeg {
                items: Some(vec![
                    crate::types::BedrockValue::String("bone".to_string()),
                    crate::types::BedrockValue::String("porkchop".to_string()),
                    crate::types::BedrockValue::String("cooked_porkchop".to_string()),
                    crate::types::BedrockValue::String("chicken".to_string()),
                    crate::types::BedrockValue::String("cooked_chicken".to_string()),
                    crate::types::BedrockValue::String("beef".to_string()),
                    crate::types::BedrockValue::String("cooked_beef".to_string()),
                    crate::types::BedrockValue::String("rotten_flesh".to_string()),
                    crate::types::BedrockValue::String("muttonraw".to_string()),
                    crate::types::BedrockValue::String("muttoncooked".to_string()),
                    crate::types::BedrockValue::String("rabbit".to_string()),
                    crate::types::BedrockValue::String("cooked_rabbit".to_string()),
                ]),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorBegPriority {}),
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
            behavior_leap_at_target: super::super::components::BehaviorLeapAtTarget {
                must_be_on_ground: Some(true),
                priority: Some(BehaviorLeapAtTargetPriority {}),
                set_persistent: Some(false),
                target_dist: None,
                yd: Some(0.4f32),
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
                track_target: Some(false),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
            },
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {}),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(vec![
                    "campfire".to_string(),
                    "fire".to_string(),
                    "fire_tick".to_string(),
                    "freezing".to_string(),
                    "lightning".to_string(),
                    "lava".to_string(),
                    "magma".to_string(),
                    "temperature".to_string(),
                    "soul_campfire".to_string(),
                ]),
                force: Some(false),
                ignore_mob_damage: Some(true),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stay_while_sitting: super::super::components::BehaviorStayWhileSitting {
                priority: Some(BehaviorStayWhileSittingPriority {}),
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
                height: Some(0.8f32),
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
            environment_sensor: super::super::components::EnvironmentSensor {
                triggers: Some(crate::types::BedrockValue::Array(vec![
                    crate::types::BedrockValue::Object(std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String(
                                "minecraft:increase_max_health".to_string(),
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
                                                        "minecraft:has_increased_max_health"
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
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "has_component".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:is_tamed".to_string(),
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
                                "minecraft:become_armorable".to_string(),
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
                                                        "minecraft:is_armorable".to_string(),
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
                                                        "minecraft:is_baby".to_string(),
                                                    ),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "has_component".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "minecraft:is_tamed".to_string(),
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
                                "minecraft:upgrade_to_1_21_100".to_string(),
                            ),
                        ),
                        (
                            "filters".to_string(),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "domain".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:was_upgraded_to_1_21_100".to_string(),
                                    ),
                                ),
                                (
                                    "operator".to_string(),
                                    crate::types::BedrockValue::String("!=".to_string()),
                                ),
                                (
                                    "test".to_string(),
                                    crate::types::BedrockValue::String("bool_property".to_string()),
                                ),
                            ])),
                        ),
                    ])),
                ])),
            },
            healable: super::super::components::Healable {
                filters: None,
                force_use: Some(false),
                items: Some(vec![
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(6i32),
                        item: Some(crate::types::BedrockValue::String("porkchop".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(16i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_porkchop".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(4i32),
                        item: Some(crate::types::BedrockValue::String("fish".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(4i32),
                        item: Some(crate::types::BedrockValue::String("salmon".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(2i32),
                        item: Some(crate::types::BedrockValue::String("clownfish".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(2i32),
                        item: Some(crate::types::BedrockValue::String("pufferfish".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(10i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_fish".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(12i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_salmon".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(6i32),
                        item: Some(crate::types::BedrockValue::String("beef".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(16i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_beef".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(4i32),
                        item: Some(crate::types::BedrockValue::String("chicken".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(12i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_chicken".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(4i32),
                        item: Some(crate::types::BedrockValue::String("muttonRaw".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(12i32),
                        item: Some(crate::types::BedrockValue::String(
                            "muttonCooked".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(8i32),
                        item: Some(crate::types::BedrockValue::String(
                            "rotten_flesh".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(6i32),
                        item: Some(crate::types::BedrockValue::String("rabbit".to_string())),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(10i32),
                        item: Some(crate::types::BedrockValue::String(
                            "cooked_rabbit".to_string(),
                        )),
                        result_item: None,
                    },
                    HealableItems {
                        effects: None,
                        filters: None,
                        heal_amount: Some(20i32),
                        item: Some(crate::types::BedrockValue::String(
                            "rabbit_stew".to_string(),
                        )),
                        result_item: None,
                    },
                ]),
            },
            health: super::super::components::Health {
                max: Some(8f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(8f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
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
            type_family: super::super::components::TypeFamily {
                family: vec!["wolf".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WolfComponentGroup {
    OnTameCollarColor,
    WolfAdult,
    WolfAngry,
    WolfArmorable,
    WolfAshen,
    WolfBaby,
    WolfBlack,
    WolfChestnut,
    WolfIncreasedMaxHealth,
    WolfLeashable,
    WolfPale,
    WolfRusty,
    WolfSnowy,
    WolfSpotted,
    WolfStriped,
    WolfTame,
    WolfWild,
    WolfWoods,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WolfEvent {
    AgeableGrowUp,
    AgeableSetBaby,
    BecomeAngry,
    BecomeArmorable,
    EntityBorn,
    EntitySpawned,
    IncreaseMaxHealth,
    OnCalm,
    OnTame,
    RandomizeSoundVariant,
    SpawnTameAdult,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildAshen,
    SpawnWildBaby,
    SpawnWildBabyOrAdult,
    SpawnWildBlack,
    SpawnWildChestnut,
    SpawnWildPale,
    SpawnWildRusty,
    SpawnWildSnowy,
    SpawnWildSpotted,
    SpawnWildStriped,
    SpawnWildWoods,
    UpgradeTo121100,
}
