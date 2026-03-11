//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:snow_golem`
pub struct SnowGolem;
impl SnowGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:snow_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:snow_golem`
#[derive(Bundle, Clone)]
pub struct SnowGolemBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_ranged_attack: super::super::components::BehaviorRangedAttack,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub interact: super::super::components::Interact,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub persistent: super::super::components::Persistent,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub shooter: super::super::components::Shooter,
    pub trail: super::super::components::Trail,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:snow_golem` entity with default Bedrock components
pub fn spawn_snow_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(SnowGolemBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
                                    crate::types::BedrockValue::String("monster".to_string()),
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
                        within_default: Some(6f32),
                    }]),
                    must_reach: Some(false),
                    must_see: Some(false),
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
            behavior_ranged_attack: super::super::components::BehaviorRangedAttack {
                attack_interval: Some(1f32),
                attack_interval_max: Some(0f32),
                attack_interval_min: Some(0f32),
                attack_radius: Some(10f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(0f32),
                charge_shoot_trigger: Some(0f32),
                priority: Some(BehaviorRangedAttackPriority {}),
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(BehaviorRangedAttackSpeedMultiplier {}),
                swing: Some(false),
                target_in_sight_time: Some(1f32),
                x_max_rotation: Some(30f32),
                y_max_head_rotation: Some(30f32),
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
                width: Some(0.4f32),
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
            health: super::super::components::Health {
                max: Some(4f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(4f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            interact: super::super::components::Interact {
                interactions: Some(vec![InteractInteractions {
                    add_items: None,
                    admire: None,
                    barter: None,
                    cooldown: Some(2.5f32),
                    cooldown_after_being_attacked: None,
                    drop_item_slot: None,
                    drop_item_y_offset: None,
                    equip_item_slot: None,
                    give_item: None,
                    health_amount: None,
                    hurt_item: Some(1i32),
                    interact_text: Some("action.interact.shear".to_string()),
                    on_interact: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "event".to_string(),
                                crate::types::BedrockValue::String(
                                    "minecraft:on_sheared".to_string(),
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
                                                            "shears".to_string(),
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
                                                        "test".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "has_component".to_string(),
                                                        ),
                                                    ),
                                                    (
                                                        "value".to_string(),
                                                        crate::types::BedrockValue::String(
                                                            "minecraft:is_sheared".to_string(),
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
                    play_sounds: Some("shear".to_string()),
                    repair_entity_item: None,
                    spawn_entities: None,
                    spawn_items: Some(InteractInteractionsSpawnItems {
                        table: Some("loot_tables/entities/snow_golem_shear.json".to_string()),
                        y_offset: None,
                    }),
                    swing: None,
                    take_item: None,
                    transform_to_item: None,
                    use_item: Some(false),
                    vibration: Some("shear".to_string()),
                }]),
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
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/snowman.json".to_string(),
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
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            shooter: super::super::components::Shooter {
                aux_val: Some(-1i32),
                def: Some("minecraft:snowball".to_string()),
                magic: Some(false),
                power: Some(0f32),
                projectiles: None,
                sound: None,
            },
            trail: super::super::components::Trail {
                block_type: Some("minecraft:snow_layer".to_string()),
                spawn_filter: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "operator".to_string(),
                            crate::types::BedrockValue::String("<".to_string()),
                        ),
                        (
                            "test".to_string(),
                            crate::types::BedrockValue::String("is_temperature_value".to_string()),
                        ),
                        (
                            "value".to_string(),
                            crate::types::BedrockValue::Float(0.81f64),
                        ),
                    ]),
                )),
                spawn_offset: Some(vec![0f32, 0f32, 0f32]),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["snowgolem".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowGolemComponentGroup {
    SnowmanSheared,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowGolemEvent {
    OnSheared,
}
