//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:piglin_brute`
pub struct PiglinBrute;
impl PiglinBrute {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:piglin_brute";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:piglin_brute`
#[derive(Bundle, Clone)]
pub struct PiglinBruteBundle {
    pub annotation_open_door: super::super::components::AnnotationOpenDoor,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:piglin_brute` entity with default Bedrock components
pub fn spawn_piglin_brute(commands: &mut Commands) -> Entity {
    commands
        .spawn(PiglinBruteBundle {
            annotation_open_door: super::super::components::AnnotationOpenDoor,
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(8f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
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
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            despawn: super::super::components::Despawn {
                despawn_from_chance: Some(true),
                despawn_from_distance: None,
                despawn_from_inactivity: Some(true),
                despawn_from_simulation_edge: Some(true),
                filters: Some(crate::types::BedrockValue::Object(
                    std::collections::HashMap::from([(
                        "any_of".to_string(),
                        crate::types::BedrockValue::Array(vec![
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "filter".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "is_persistent".to_string(),
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
                                                    crate::types::BedrockValue::String(
                                                        ">".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "distance_to_nearest_player".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(54i64),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                            crate::types::BedrockValue::Object(std::collections::HashMap::from([
                                (
                                    "filter".to_string(),
                                    crate::types::BedrockValue::Array(vec![
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "is_persistent".to_string(),
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
                                                    "subject".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "self".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "inactivity_timer".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(30i64),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "random_chance".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(800i64),
                                                ),
                                            ]),
                                        ),
                                        crate::types::BedrockValue::Object(
                                            std::collections::HashMap::from([
                                                (
                                                    "operator".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        ">".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "test".to_string(),
                                                    crate::types::BedrockValue::String(
                                                        "distance_to_nearest_player".to_string(),
                                                    ),
                                                ),
                                                (
                                                    "value".to_string(),
                                                    crate::types::BedrockValue::Integer(32i64),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                ),
                            ])),
                        ]),
                    )]),
                )),
                min_range_inactivity_timer: Some(30i32),
                min_range_random_chance: Some(800i32),
                remove_child_entities: Some(false),
            },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 20 : 0".to_string(),
                )),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(50f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(50f32),
            },
            home: super::super::components::Home {
                home_block_list: None,
                restriction_radius: Some(0i32),
                restriction_type: Some("none".to_string()),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/piglin.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.35f32),
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
                avoid_water: Some(false),
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
            on_target_acquired: super::super::components::OnTargetAcquired {
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
                    "piglin".to_string(),
                    "adult_piglin".to_string(),
                    "piglin_brute".to_string(),
                    "monster".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiglinBruteComponentGroup {
    AlertForAttackTargets,
    Angry,
    BecomeZombie,
    GoBackToSpawn,
    MeleeUnit,
    StartZombification,
    TakeTargetAsResponseToBlockBreak,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiglinBruteEvent {
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    GoBackToSpawnFailed,
    ImportantBlockDestroyedEvent,
    EntitySpawned,
    StartZombificationEvent,
    StopZombificationEvent,
}
