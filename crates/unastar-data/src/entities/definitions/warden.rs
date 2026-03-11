//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:warden`
pub struct Warden;
impl Warden {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:warden";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:warden`
#[derive(Bundle, Clone)]
pub struct WardenBundle {
    pub ambient_sound_interval: super::super::components::AmbientSoundInterval,
    pub anger_level: super::super::components::AngerLevel,
    pub attack: super::super::components::Attack,
    pub behavior_dig: super::super::components::BehaviorDig,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_investigate_suspicious_location:
        super::super::components::BehaviorInvestigateSuspiciousLocation,
    pub behavior_melee_box_attack: super::super::components::BehaviorMeleeBoxAttack,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_roar: super::super::components::BehaviorRoar,
    pub behavior_sniff: super::super::components::BehaviorSniff,
    pub behavior_sonic_boom: super::super::components::BehaviorSonicBoom,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub experience_reward: super::super::components::ExperienceReward,
    pub fire_immune: super::super::components::FireImmune,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub heartbeat: super::super::components::Heartbeat,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub knockback_resistance: super::super::components::KnockbackResistance,
    pub loot: super::super::components::Loot,
    pub mob_effect: super::super::components::MobEffect,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub movement_sound_distance_offset: super::super::components::MovementSoundDistanceOffset,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub preferred_path: super::super::components::PreferredPath,
    pub pushable: super::super::components::Pushable,
    pub suspect_tracking: super::super::components::SuspectTracking,
    pub type_family: super::super::components::TypeFamily,
    pub vibration_damper: super::super::components::VibrationDamper,
    pub vibration_listener: super::super::components::VibrationListener,
}
/// Spawn a new `minecraft:warden` entity with default Bedrock components
pub fn spawn_warden(commands: &mut Commands) -> Entity {
    commands
        .spawn(WardenBundle {
            ambient_sound_interval: super::super::components::AmbientSoundInterval {
                event_name: Some("ambient".to_string()),
                event_names: Some(vec![
                    AmbientSoundIntervalEventNames {
                        condition: Some("query.anger_level(this) >= 80".to_string()),
                        event_name: Some("angry".to_string()),
                    },
                    AmbientSoundIntervalEventNames {
                        condition: Some("query.anger_level(this) >= 40".to_string()),
                        event_name: Some("agitated".to_string()),
                    },
                ]),
                range: Some(4f32),
                value: 2f32,
            },
            anger_level: super::super::components::AngerLevel {
                anger_decrement_interval: Some(1f32),
                angry_boost: Some(20i32),
                angry_threshold: Some(80i32),
                default_annoyingness: Some(35f32),
                default_projectile_annoyingness: Some(10f32),
                max_anger: Some(150i32),
                nuisance_filter: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "filter".to_string(),
                        crate::types::BedrockValue::Array(vec![
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
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
                                    crate::types::BedrockValue::String("warden".to_string()),
                                ),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
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
                                    crate::types::BedrockValue::String("inanimate".to_string()),
                                ),
                            ])),
                        ]),
                    )]),
                )),
                on_increase_sounds: Some(vec![
                    AngerLevelOnIncreaseSounds {
                        condition: "query.anger_level(this) >= 40".to_string(),
                        sound: "listening_angry".to_string(),
                    },
                    AngerLevelOnIncreaseSounds {
                        condition: "query.anger_level(this) >= 0".to_string(),
                        sound: "listening".to_string(),
                    },
                ]),
                remove_targets_below_angry_threshold: Some(true),
            },
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(30f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_dig: super::super::components::BehaviorDig {
                allow_dig_when_named: Some(false),
                control_flags: Some(BehaviorDigControlFlags {}),
                digs_in_daylight: Some(false),
                duration: Some(5.5f32),
                idle_time: Some(60f32),
                on_start: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([
                        (
                            "event".to_string(),
                            crate::types::BedrockValue::String("on_digging_event".to_string()),
                        ),
                        (
                            "target".to_string(),
                            crate::types::BedrockValue::String("self".to_string()),
                        ),
                    ]),
                )),
                priority: Some(BehaviorDigPriority {}),
                suspicion_is_disturbance: Some(true),
                vibration_is_disturbance: Some(true),
            },
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_investigate_suspicious_location:
                super::super::components::BehaviorInvestigateSuspiciousLocation {
                    control_flags: Some(BehaviorInvestigateSuspiciousLocationControlFlags {}),
                    goal_radius: Some(1.5f32),
                    priority: Some(5i32),
                    speed_multiplier: Some(0.7f32),
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
                melee_fov: Some(360f32),
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
            behavior_roar: super::super::components::BehaviorRoar {
                control_flags: Some(BehaviorRoarControlFlags {}),
                duration: Some(4.2f32),
                priority: Some(BehaviorRoarPriority {}),
            },
            behavior_sniff: super::super::components::BehaviorSniff {
                control_flags: Some(BehaviorSniffControlFlags {}),
                cooldown_range: Some(vec![0f32]),
                duration: Some(4.16f32),
                priority: Some(BehaviorSniffPriority {}),
                sniffing_radius: Some(24f32),
                suspicion_radius_horizontal: Some(6f32),
                suspicion_radius_vertical: Some(20f32),
            },
            behavior_sonic_boom: super::super::components::BehaviorSonicBoom {
                attack_cooldown: Some(2f32),
                attack_damage: Some(10f32),
                attack_range_horizontal: Some(15f32),
                attack_range_vertical: Some(20f32),
                attack_sound: Some("sonic_boom".to_string()),
                charge_sound: Some("sonic_charge".to_string()),
                control_flags: Some(BehaviorSonicBoomControlFlags {}),
                duration: Some(3f32),
                duration_until_attack_sound: Some(1.7f32),
                knockback_height_cap: Some(0.5f32),
                knockback_horizontal_strength: Some(2.5f32),
                knockback_vertical_strength: Some(0.5f32),
                priority: Some(BehaviorSonicBoomPriority {}),
                speed_multiplier: Some(BehaviorSonicBoomSpeedMultiplier {}),
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
                height: Some(2.9f32),
                width: Some(0.9f32),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Expr("Math.Random(1,7)".to_string())),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 5 : 0".to_string(),
                )),
            },
            fire_immune: super::super::components::FireImmune,
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(30f32),
            },
            health: super::super::components::Health {
                max: Some(500f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(500f32),
            },
            heartbeat: super::super::components::Heartbeat {
                interval: Some(crate::types::MolangOr::Expr(
                    "2.0 - math.clamp(query.anger_level / 80 * 1.5, 0, 1.5)".to_string(),
                )),
                sound_event: Some("heartbeat".to_string()),
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
            loot: super::super::components::Loot {
                table: "loot_tables/entities/warden.json".to_string(),
            },
            mob_effect: super::super::components::MobEffect {
                ambient: Some(false),
                cooldown_time: Some(6i32),
                effect_range: Some(20f32),
                effect_time: Some(crate::types::MolangOr::Value(13i32)),
                entity_filter: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "filter".to_string(),
                        crate::types::BedrockValue::Array(vec![
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
                                    crate::types::BedrockValue::String("player".to_string()),
                                ),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
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
                                    crate::types::BedrockValue::String("has_ability".to_string()),
                                ),
                                (
                                    "value".to_string(),
                                    crate::types::BedrockValue::String("invulnerable".to_string()),
                                ),
                            ])),
                        ]),
                    )]),
                )),
                mob_effect: Some("darkness".to_string()),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
            },
            movement_sound_distance_offset: super::super::components::MovementSoundDistanceOffset {
                value: 0.55f32,
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
                can_path_over_lava: Some(true),
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
            preferred_path: super::super::components::PreferredPath {
                default_block_cost: Some(0f32),
                jump_cost: Some(0i32),
                max_fall_blocks: Some(20i32),
                preferred_path_blocks: None,
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            suspect_tracking: super::super::components::SuspectTracking,
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "warden".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
            vibration_damper: super::super::components::VibrationDamper,
            vibration_listener: super::super::components::VibrationListener,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WardenComponentGroup {
    Emerging,
    Pushable,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WardenEvent {
    Emerged,
    EntitySpawned,
    SpawnEmerging,
    OnDiggingEvent,
}
