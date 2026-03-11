//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:axolotl`
pub struct Axolotl;
impl Axolotl {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:axolotl";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:axolotl`
#[derive(Bundle, Clone)]
pub struct AxolotlBundle {
    pub attack: super::super::components::Attack,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack,
    pub behavior_move_to_water: super::super::components::BehaviorMoveToWater,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_play_dead: super::super::components::BehaviorPlayDead,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_random_swim: super::super::components::BehaviorRandomSwim,
    pub behavior_swim_idle: super::super::components::BehaviorSwimIdle,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub combat_regeneration: super::super::components::CombatRegeneration,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_amphibious: super::super::components::MovementAmphibious,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub underwater_movement: super::super::components::UnderwaterMovement,
}
/// Spawn a new `minecraft:axolotl` entity with default Bedrock components
pub fn spawn_axolotl(commands: &mut Commands) -> Entity {
    commands
        .spawn(AxolotlBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
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
                on_kill: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String("killed_enemy_event".to_string()),
                        ),
                        (
                            "target".to_string(),
                            crate::types::BedrockValue::String("self".to_string()),
                        ),
                    ]),
                )),
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
            behavior_move_to_water: super::super::components::BehaviorMoveToWater {
                goal_radius: Some(0.1f32),
                priority: Some(BehaviorMoveToWaterPriority {}),
                search_count: Some(1i32),
                search_height: Some(5i32),
                search_range: Some(16i32),
                speed_multiplier: Some(BehaviorMoveToWaterSpeedMultiplier {}),
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
                                        crate::types::BedrockValue::Object(
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
                                                                    "squid".to_string(),
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
                                                                    "fish".to_string(),
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
                                                                    "tadpole".to_string(),
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
                            max_dist: Some(8f32),
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
                                                                    "drowned".to_string(),
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
                                                                    "guardian".to_string(),
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
                                                                    "guardian_elder".to_string(),
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
                            max_dist: Some(8f32),
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
                    must_see_forget_duration: Some(17f32),
                    persist_time: Some(0f32),
                    priority: Some(BehaviorNearestAttackableTargetPriority {}),
                    reselect_targets: Some(true),
                    scan_interval: Some(10i32),
                    set_persistent: Some(false),
                    target_acquisition_probability: Some(1f32),
                    target_invisible_multiplier: Some(0.7f32),
                    target_search_height: Some(-1f32),
                    target_sneak_visibility_multiplier: Some(0.8f32),
                    within_radius: Some(20f32),
                },
            behavior_play_dead: super::super::components::BehaviorPlayDead {
                apply_regeneration: Some(true),
                damage_sources: Some(vec![
                    "contact".to_string(),
                    "entity_attack".to_string(),
                    "entity_explosion".to_string(),
                    "magic".to_string(),
                    "projectile".to_string(),
                    "thorns".to_string(),
                    "wither".to_string(),
                ]),
                duration: Some(10f32),
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "operator".to_string(),
                            crate::types::BedrockValue::String("==".to_string()),
                        ),
                        (
                            "test".to_string(),
                            crate::types::BedrockValue::String("in_water".to_string()),
                        ),
                        ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                    ]),
                )),
                force_below_health: Some(8i32),
                priority: Some(BehaviorPlayDeadPriority {}),
                random_damage_range: None,
                random_start_chance: Some(0.33f32),
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(100i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_random_swim: super::super::components::BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(BehaviorRandomSwimPriority {}),
                speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {}),
                xz_dist: Some(30i32),
                y_dist: Some(15i32),
            },
            behavior_swim_idle: super::super::components::BehaviorSwimIdle {
                control_flags: Some(BehaviorSwimIdleControlFlags {}),
                idle_time: Some(5f32),
                priority: Some(BehaviorSwimIdlePriority {}),
                success_rate: Some(0.05f32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(true),
                can_tempt_while_ridden: Some(false),
                items: Some(vec![crate::types::BedrockValue::String(
                    "tropical_fish_bucket".to_string(),
                )]),
                on_end: None,
                on_start: None,
                priority: Some(BehaviorTemptPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
                stop_distance: Some(1.5f32),
                tempt_sound: None,
                within_radius: Some(0f32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(false),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.42f32),
                width: Some(0.75f32),
            },
            combat_regeneration: super::super::components::CombatRegeneration {
                apply_to_family: Some(false),
                apply_to_self: Some(false),
                regeneration_duration: Some(crate::types::MolangOr::Value(5i32)),
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(vec![DamageSensorTriggers {
                    cause: Some("lightning".to_string()),
                    damage_modifier: None,
                    damage_multiplier: Some(2000f32),
                    deals_damage: Some("yes".to_string()),
                    on_damage: None,
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
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(14f32),
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
                value: crate::types::RangeOrVal::Fixed(0.1f32),
            },
            movement_amphibious: super::super::components::MovementAmphibious {
                max_turn: Some(15f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_generic: super::super::components::NavigationGeneric {
                avoid_damage_blocks: Some(true),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(false),
                blocks_to_avoid: None,
                can_breach: Some(false),
                can_break_doors: Some(false),
                can_jump: Some(true),
                can_open_doors: Some(false),
                can_open_iron_doors: Some(false),
                can_pass_doors: Some(true),
                can_path_from_air: Some(false),
                can_path_over_lava: Some(false),
                can_path_over_water: Some(true),
                can_sink: Some(false),
                can_swim: Some(true),
                can_walk: Some(true),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(true),
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
                family: vec![
                    "aquatic".to_string(),
                    "axolotl".to_string(),
                    "mob".to_string(),
                ],
            },
            underwater_movement: super::super::components::UnderwaterMovement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.2f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxolotlComponentGroup {
    AttackCooldown,
    AxolotlAdult,
    AxolotlBaby,
    AxolotlBlue,
    AxolotlCyan,
    AxolotlDried,
    AxolotlGold,
    AxolotlInWater,
    AxolotlLucy,
    AxolotlOnLand,
    AxolotlOnLandInRain,
    AxolotlWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxolotlEvent {
    AttackCooldownCompleteEvent,
    DriedOut,
    EnterWater,
    KilledEnemyEvent,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    RecoverAfterDriedOut,
    StartDryingOut,
    StopDryingOut,
}
