//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:slime`
pub struct Slime;
impl Slime {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:slime";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:slime`
#[derive(Bundle, Clone)]
pub struct SlimeBundle {
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_slime_attack: super::super::components::BehaviorSlimeAttack,
    pub behavior_slime_float: super::super::components::BehaviorSlimeFloat,
    pub behavior_slime_keep_on_jumping: super::super::components::BehaviorSlimeKeepOnJumping,
    pub behavior_slime_random_direction: super::super::components::BehaviorSlimeRandomDirection,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub mob_effect_immunity: super::super::components::MobEffectImmunity,
    pub movement_jump: super::super::components::MovementJump,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub on_target_acquired: super::super::components::OnTargetAcquired,
    pub on_target_escape: super::super::components::OnTargetEscape,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:slime` entity with default Bedrock components
pub fn spawn_slime(commands: &mut Commands) -> Entity {
    commands
        .spawn(SlimeBundle {
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
                                ]),
                            )]),
                        )),
                        max_dist: Some(16f32),
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
            behavior_slime_attack: super::super::components::BehaviorSlimeAttack {
                control_flags: Some(BehaviorSlimeAttackControlFlags {}),
                grow_tired_cooldown_time: Some(15f32),
                priority: Some(BehaviorSlimeAttackPriority {}),
                set_persistent: Some(false),
                speed_multiplier: Some(BehaviorSlimeAttackSpeedMultiplier {}),
                x_max_rotation: Some(10f32),
                y_max_rotation: Some(10f32),
            },
            behavior_slime_float: super::super::components::BehaviorSlimeFloat {
                control_flags: Some(BehaviorSlimeFloatControlFlags {}),
                jump_chance_percentage: Some(0.8f32),
                priority: Some(BehaviorSlimeFloatPriority {}),
                speed_multiplier: Some(BehaviorSlimeFloatSpeedMultiplier {}),
            },
            behavior_slime_keep_on_jumping: super::super::components::BehaviorSlimeKeepOnJumping {
                control_flags: Some(BehaviorSlimeKeepOnJumpingControlFlags {}),
                priority: Some(BehaviorSlimeKeepOnJumpingPriority {}),
                speed_multiplier: Some(BehaviorSlimeKeepOnJumpingSpeedMultiplier {}),
            },
            behavior_slime_random_direction:
                super::super::components::BehaviorSlimeRandomDirection {
                    add_random_time_range: Some(3i32),
                    control_flags: Some(BehaviorSlimeRandomDirectionControlFlags {}),
                    min_change_direction_time: Some(2f32),
                    priority: Some(BehaviorSlimeRandomDirectionPriority {}),
                    turn_range: Some(360i32),
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
                height: Some(2.08f32),
                width: Some(2.08f32),
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
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? query.variant : 0".to_string(),
                )),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            mob_effect_immunity: super::super::components::MobEffectImmunity {
                mob_effects: Some(vec!["oozing".to_string()]),
            },
            movement_jump: super::super::components::MovementJump {
                jump_delay: None,
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "slime".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlimeComponentGroup {
    SlimeAggressive,
    SlimeCalm,
    SlimeLarge,
    SlimeMedium,
    SlimeSmall,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlimeEvent {
    BecomeAggressive,
    BecomeCalm,
    EntitySpawned,
    SpawnLarge,
    SpawnMedium,
    SpawnSmall,
}
