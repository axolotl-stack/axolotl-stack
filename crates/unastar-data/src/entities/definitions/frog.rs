//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:frog`
pub struct Frog;
impl Frog {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:frog";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:frog`
#[derive(Bundle, Clone)]
pub struct FrogBundle {
    pub behavior_breed: super::super::components::BehaviorBreed,
    pub behavior_croak: super::super::components::BehaviorCroak,
    pub behavior_eat_mob: super::super::components::BehaviorEatMob,
    pub behavior_jump_to_block: super::super::components::BehaviorJumpToBlock,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_move_to_land: super::super::components::BehaviorMoveToLand,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub breedable: super::super::components::Breedable,
    pub collision_box: super::super::components::CollisionBox,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub leashable_to: super::super::components::LeashableTo,
    pub movement: super::super::components::Movement,
    pub movement_amphibious: super::super::components::MovementAmphibious,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub underwater_movement: super::super::components::UnderwaterMovement,
}
/// Spawn a new `minecraft:frog` entity with default Bedrock components
pub fn spawn_frog(commands: &mut Commands) -> Entity {
    commands
        .spawn(FrogBundle {
            behavior_breed: super::super::components::BehaviorBreed {
                priority: Some(BehaviorBreedPriority {}),
                speed_multiplier: Some(BehaviorBreedSpeedMultiplier {}),
            },
            behavior_croak: super::super::components::BehaviorCroak {
                duration: Some(vec![4.5f32]),
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "all_of".to_string(),
                                crate::types::BedrockValue::Array(
                                    vec![
                                        crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate ::types::BedrockValue::String("in_water"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))])), crate
                                        ::types::BedrockValue::Object(std::collections::HashMap::from([("test"
                                        .to_string(), crate ::types::BedrockValue::String("in_lava"
                                        .to_string())), ("value".to_string(), crate
                                        ::types::BedrockValue::Bool(false))]))
                                    ],
                                ),
                            ),
                        ]),
                    ),
                ),
                interval: None,
                priority: Some(BehaviorCroakPriority {}),
            },
            behavior_eat_mob: super::super::components::BehaviorEatMob {
                eat_animation_time: Some(0.3f32),
                eat_mob_sound: Some("tongue".to_string()),
                loot_table: Some("loot_tables/entities/frog.json".to_string()),
                priority: Some(BehaviorEatMobPriority {}),
                pull_in_force: Some(0.75f32),
                reach_mob_distance: Some(1.75f32),
                run_speed: Some(2f32),
            },
            behavior_jump_to_block: super::super::components::BehaviorJumpToBlock {
                cooldown_range: None,
                forbidden_blocks: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:water"
                        .to_string())
                    ],
                ),
                max_velocity: Some(1f32),
                minimum_distance: Some(1i32),
                minimum_path_length: Some(2i32),
                preferred_blocks: Some(
                    vec![
                        crate ::types::BedrockValue::String("minecraft:waterlily"
                        .to_string()), crate
                        ::types::BedrockValue::String("minecraft:big_dripleaf"
                        .to_string())
                    ],
                ),
                preferred_blocks_chance: Some(0.5f32),
                priority: Some(BehaviorJumpToBlockPriority {}),
                scale_factor: Some(0.6f32),
                search_height: Some(4i32),
                search_width: Some(8i32),
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
            behavior_move_to_land: super::super::components::BehaviorMoveToLand {
                goal_radius: Some(2f32),
                priority: Some(BehaviorMoveToLandPriority {}),
                search_count: Some(80i32),
                search_height: Some(8i32),
                search_range: Some(30i32),
                speed_multiplier: Some(BehaviorMoveToLandSpeedMultiplier {
                }),
            },
            behavior_nearest_attackable_target: super::super::components::BehaviorNearestAttackableTarget {
                attack_interval: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "max".to_string(),
                                crate::types::BedrockValue::Integer(0i64),
                            ),
                            (
                                "min".to_string(),
                                crate::types::BedrockValue::Integer(0i64),
                            ),
                        ]),
                    ),
                ),
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
                        .to_string(), crate ::types::BedrockValue::String("slime"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("=="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("is_variant"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Integer(1i64))]))]))]))), max_dist :
                        Some(16f32), max_flee : None, max_height : None, must_see : None,
                        must_see_forget_duration : None, priority : None,
                        reevaluate_description : None, sprint_speed_multiplier : None,
                        walk_speed_multiplier : None, within_default : None },
                        BehaviorNearestAttackableTargetEntityTypes { check_if_outnumbered
                        : None, cooldown : None, filters : Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("magmacube"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("=="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("is_variant"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Integer(1i64))]))]))]))), max_dist :
                        Some(16f32), max_flee : None, max_height : None, must_see : None,
                        must_see_forget_duration : None, priority : None,
                        reevaluate_description : None, sprint_speed_multiplier : None,
                        walk_speed_multiplier : None, within_default : None }
                    ],
                ),
                must_reach: Some(false),
                must_see: Some(true),
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
                within_radius: Some(16f32),
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
            behavior_random_stroll: super::super::components::BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(BehaviorRandomStrollPriority {}),
                speed_multiplier: Some(BehaviorRandomStrollSpeedMultiplier {
                }),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(true),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![crate ::types::BedrockValue::String("slime_ball".to_string())],
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
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(false),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(15i32),
            },
            breedable: super::super::components::Breedable {
                allow_sitting: Some(false),
                breed_cooldown: Some(60f32),
                breed_items: Some(
                    vec![crate ::types::BedrockValue::String("slime_ball".to_string())],
                ),
                breeds_with: Some(
                    vec![
                        BreedableBreedsWith { baby_type : Some("minecraft:tadpole"
                        .to_string()), breed_event : Some(BreedableBreedsWithBreedEvent {
                        event : Some("become_pregnant".to_string()), filters : None,
                        target : None }), mate_type : Some("minecraft:frog".to_string())
                        }
                    ],
                ),
                causes_pregnancy: Some(true),
                environment_requirements: None,
                extra_baby_chance: Some(0f32),
                love_filters: None,
                require_full_health: Some(false),
                require_tame: Some(false),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.55f32),
                width: Some(0.5f32),
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : Some("fall".to_string()),
                        damage_modifier : Some(- 5f32), damage_multiplier : None,
                        deals_damage : Some("yes".to_string()), on_damage : None,
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(
                    crate::types::MolangOr::Expr("Math.Random(1,7)".to_string()),
                ),
                on_death: Some(
                    crate::types::MolangOr::Expr(
                        "query.last_hit_by_player ? Math.Random(1,3) : 0".to_string(),
                    ),
                ),
            },
            health: super::super::components::Health {
                max: None,
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
            leashable_to: super::super::components::LeashableTo {
                can_retrieve_from: Some(false),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.1f32),
            },
            movement_amphibious: super::super::components::MovementAmphibious {
                max_turn: Some(30f32),
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
                family: vec!["frog".to_string(), "mob".to_string()],
            },
            underwater_movement: super::super::components::UnderwaterMovement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.15f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogComponentGroup {
    ColdFrog,
    Pregnant,
    TemperateFrog,
    WarmFrog,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogEvent {
    BecomePregnant,
    LaidEgg,
    EntitySpawned,
    EntityTransformed,
    SpawnCold,
    SpawnTemperate,
    SpawnWarm,
}
