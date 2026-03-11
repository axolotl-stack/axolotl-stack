//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:llama`
pub struct Llama;
impl Llama {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:llama";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:llama`
#[derive(Bundle, Clone)]
pub struct LlamaBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_mount_pathing: super::super::components::BehaviorMountPathing,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_run_around_like_crazy: super::super::components::BehaviorRunAroundLikeCrazy,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub follow_range: super::super::components::FollowRange,
    pub healable: super::super::components::Healable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub mark_variant: super::super::components::MarkVariant,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub on_target_escape: super::super::components::OnTargetEscape,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub shooter: super::super::components::Shooter,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:llama` entity with default Bedrock components
pub fn spawn_llama(commands: &mut Commands) -> Entity {
    commands
        .spawn(LlamaBundle {
            balloonable: super::super::components::Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
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
                hurt_owner: Some(true),
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
            behavior_mount_pathing: super::super::components::BehaviorMountPathing {
                priority: Some(BehaviorMountPathingPriority {}),
                speed_multiplier: Some(BehaviorMountPathingSpeedMultiplier {
                }),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_nearest_attackable_target: super::super::components::BehaviorNearestAttackableTarget {
                attack_interval: Some(crate::types::BedrockValue::Integer(16i64)),
                attack_interval_min: None,
                attack_owner: Some(false),
                control_flags: Some(BehaviorNearestAttackableTargetControlFlags {
                }),
                entity_types: Some(
                    vec![
                        BehaviorNearestAttackableTargetEntityTypes { check_if_outnumbered
                        : None, cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("wolf"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("!="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("has_component"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::String("minecraft:is_tamed"
                        .to_string()))]))]))]))), max_dist : Some(10f32), max_flee :
                        None, max_height : None, must_see : None,
                        must_see_forget_duration : None, priority : None,
                        reevaluate_description : None, sprint_speed_multiplier : None,
                        walk_speed_multiplier : None, within_default : None }
                    ],
                ),
                must_reach: Some(true),
                must_see: Some(false),
                must_see_forget_duration: Some(3f32),
                persist_time: Some(0f32),
                priority: Some(BehaviorNearestAttackableTargetPriority {
                }),
                reselect_targets: Some(false),
                scan_interval: Some(10i32),
                set_persistent: Some(false),
                target_acquisition_probability: Some(1f32),
                target_invisible_multiplier: Some(0.7f32),
                target_search_height: Some(-1f32),
                target_sneak_visibility_multiplier: Some(0.8f32),
                within_radius: Some(0f32),
            },
            behavior_panic: super::super::components::BehaviorPanic {
                damage_sources: Some(
                    vec![
                        "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                        .to_string()
                    ],
                ),
                force: Some(false),
                ignore_mob_damage: Some(false),
                panic_sound: None,
                prefer_water: Some(false),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {
                }),
                probability: None,
            },
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_run_around_like_crazy: super::super::components::BehaviorRunAroundLikeCrazy {
                priority: Some(BehaviorRunAroundLikeCrazyPriority {
                }),
                speed_multiplier: Some(BehaviorRunAroundLikeCrazySpeedMultiplier {
                }),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(false),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![crate ::types::BedrockValue::String("hay_block".to_string())],
                ),
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
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(1.87f32),
                width: Some(0.9f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : Some("all".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("yes".to_string()), on_damage : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("event"
                        .to_string(), crate
                        ::types::BedrockValue::String("minecraft:become_angry"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                        .to_string(), crate ::types::BedrockValue::String("in_caravan"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Bool(false))])))]))),
                        on_damage_sound_event : None }
                    ],
                ),
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
            follow_range: super::super::components::FollowRange {
                max: Some(40f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(40f32),
            },
            healable: super::super::components::Healable {
                filters: None,
                force_use: Some(false),
                items: Some(
                    vec![
                        HealableItems { effects : None, filters : None, heal_amount :
                        Some(2i32), item : Some(crate
                        ::types::BedrockValue::String("wheat".to_string())), result_item
                        : None }, HealableItems { effects : None, filters : None,
                        heal_amount : Some(10i32), item : Some(crate
                        ::types::BedrockValue::String("hay_block".to_string())),
                        result_item : None }
                    ],
                ),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Range {
                    min: 15f32,
                    max: 30f32,
                },
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
                on_leash: Some(LeashableOnLeash {
                    event: Some("minecraft:on_leash".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                on_unleash: Some(LeashableOnUnleash {
                    event: Some("minecraft:on_unleash".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                on_unleash_interact_only: Some(false),
                presets: None,
                soft_distance: Some(4f32),
            },
            mark_variant: super::super::components::MarkVariant {
                value: 0i32,
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
            on_target_acquired: super::super::components::OnTargetAcquired {
                value: crate::types::BedrockValue::Null,
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
            shooter: super::super::components::Shooter {
                aux_val: Some(-1i32),
                def: Some("minecraft:llama_spit".to_string()),
                magic: Some(false),
                power: Some(0f32),
                projectiles: None,
                sound: None,
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["llama".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlamaComponentGroup {
    InCaravan,
    LlamaAdult,
    LlamaAngry,
    LlamaAngryWolf,
    LlamaBaby,
    LlamaBrown,
    LlamaChested,
    LlamaCreamy,
    LlamaGray,
    LlamaTamed,
    LlamaUnchested,
    LlamaUnleashed,
    LlamaWhite,
    LlamaWild,
    Strength1,
    Strength2,
    Strength3,
    Strength4,
    Strength5,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlamaEvent {
    AddAttributes,
    AgeableGrowUp,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    JoinCaravan,
    LeaveCaravan,
    MadAtWolf,
    OnCalm,
    OnChest,
    OnLeash,
    OnTame,
    OnUnleash,
    SpawnAdult,
    SpawnBaby,
    SpawnTameAdult,
}
