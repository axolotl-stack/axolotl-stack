//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:dolphin`
pub struct Dolphin;
impl Dolphin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:dolphin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:dolphin`
#[derive(Bundle, Clone)]
pub struct DolphinBundle {
    pub attack: super::super::components::Attack,
    pub balloonable: super::super::components::Balloonable,
    pub behavior_avoid_mob_type: super::super::components::BehaviorAvoidMobType,
    pub behavior_find_underwater_treasure: super::super::components::BehaviorFindUnderwaterTreasure,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_move_to_water: super::super::components::BehaviorMoveToWater,
    pub behavior_random_breach: super::super::components::BehaviorRandomBreach,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_swim: super::super::components::BehaviorRandomSwim,
    pub behavior_swim_up_for_breath: super::super::components::BehaviorSwimUpForBreath,
    pub behavior_swim_with_entity: super::super::components::BehaviorSwimWithEntity,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub flocking: super::super::components::Flocking,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub on_target_escape: super::super::components::OnTargetEscape,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub underwater_movement: super::super::components::UnderwaterMovement,
}
/// Spawn a new `minecraft:dolphin` entity with default Bedrock components
pub fn spawn_dolphin(commands: &mut Commands) -> Entity {
    commands
        .spawn(DolphinBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(3f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            balloonable: super::super::components::Balloonable {
                mass: Some(0.4f32),
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
                                                "guardian_elder".to_string(),
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
                                            crate::types::BedrockValue::String(
                                                "guardian".to_string(),
                                            ),
                                        ),
                                    ]),
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
                    sprint_speed_multiplier: Some(1f32),
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
                probability_per_strength: Some(0.14f32),
                remove_target: Some(false),
                sound_interval: Some(crate::types::RangeOrVal::Range {
                    min: 3f32,
                    max: 8f32,
                }),
                sprint_distance: Some(7f32),
                sprint_speed_multiplier: Some(1f32),
                walk_speed_multiplier: Some(1f32),
            },
            behavior_find_underwater_treasure:
                super::super::components::BehaviorFindUnderwaterTreasure {
                    priority: Some(BehaviorFindUnderwaterTreasurePriority {}),
                    search_range: Some(30i32),
                    speed_multiplier: Some(BehaviorFindUnderwaterTreasureSpeedMultiplier {}),
                    stop_distance: Some(50f32),
                },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_move_to_water: super::super::components::BehaviorMoveToWater {
                goal_radius: Some(0.5f32),
                priority: Some(BehaviorMoveToWaterPriority {}),
                search_count: Some(10i32),
                search_height: Some(5i32),
                search_range: Some(15i32),
                speed_multiplier: Some(BehaviorMoveToWaterSpeedMultiplier {}),
            },
            behavior_random_breach: super::super::components::BehaviorRandomBreach {
                cooldown_time: Some(2f32),
                interval: Some(50i32),
                priority: Some(BehaviorRandomBreachPriority {}),
                speed_multiplier: Some(BehaviorRandomBreachSpeedMultiplier {}),
                xz_dist: Some(6i32),
                y_dist: Some(7i32),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_random_swim: super::super::components::BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(BehaviorRandomSwimPriority {}),
                speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {}),
                xz_dist: Some(20i32),
                y_dist: Some(7i32),
            },
            behavior_swim_up_for_breath: super::super::components::BehaviorSwimUpForBreath {
                control_flags: Some(BehaviorSwimUpForBreathControlFlags {}),
                material_type: Some("water".to_string()),
                priority: Some(BehaviorSwimUpForBreathPriority {}),
                search_height: Some(16i32),
                search_radius: Some(4i32),
                speed_mod: Some(1.4f32),
            },
            behavior_swim_with_entity: super::super::components::BehaviorSwimWithEntity {
                catch_up_multiplier: Some(2.5f32),
                catch_up_threshold: Some(12f32),
                chance_to_stop: Some(0.0333f32),
                entity_types: Some(vec![BehaviorSwimWithEntityEntityTypes {
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
                match_direction_threshold: Some(2f32),
                priority: Some(BehaviorSwimWithEntityPriority {}),
                search_range: Some(20f32),
                speed_multiplier: Some(BehaviorSwimWithEntitySpeedMultiplier {}),
                state_check_interval: Some(0.5f32),
                stop_distance: Some(5f32),
                success_rate: Some(0.1f32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(false),
                can_dehydrate: Some(false),
                generates_bubbles: Some(false),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(0i32),
                total_supply: Some(240i32),
            },
            can_climb: super::super::components::CanClimb,
            collision_box: super::super::components::CollisionBox {
                height: Some(0.6f32),
                width: Some(0.9f32),
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
            flocking: super::super::components::Flocking {
                block_distance: Some(1f32),
                block_weight: Some(0f32),
                breach_influence: Some(0f32),
                cohesion_threshold: Some(6.5f32),
                cohesion_weight: Some(1.85f32),
                goal_weight: Some(2f32),
                high_flock_limit: Some(8i32),
                in_water: Some(false),
                influence_radius: Some(6f32),
                innner_cohesion_threshold: Some(3.5f32),
                loner_chance: Some(0.1f32),
                low_flock_limit: Some(4i32),
                match_variants: Some(false),
                max_height: Some(4f32),
                min_height: Some(4f32),
                separation_threshold: Some(3f32),
                separation_weight: Some(1.75f32),
                use_center_of_mass: Some(false),
            },
            follow_range: super::super::components::FollowRange {
                max: Some(48f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(48f32),
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
                jump_power: Some(0.6f32),
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
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_generic: super::super::components::NavigationGeneric {
                avoid_damage_blocks: Some(false),
                avoid_portals: Some(false),
                avoid_sun: Some(false),
                avoid_water: Some(false),
                blocks_to_avoid: None,
                can_breach: Some(true),
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
                can_walk: Some(false),
                can_walk_in_lava: Some(false),
                is_amphibious: Some(true),
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "aquatic".to_string(),
                    "dolphin".to_string(),
                    "mob".to_string(),
                ],
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
pub enum DolphinComponentGroup {
    DolphinAdult,
    DolphinAngry,
    DolphinBaby,
    DolphinDried,
    DolphinOnLand,
    DolphinOnLandInRain,
    DolphinSwimmingNavigation,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DolphinEvent {
    AgeableGrowUp,
    BecomeAngry,
    DriedOut,
    EntitySpawned,
    NavigationOffLand,
    NavigationOnLand,
    OnCalm,
    RecoverAfterDriedOut,
    StartDryingout,
    StopDryingout,
}
