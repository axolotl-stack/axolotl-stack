//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:elder_guardian`
pub struct ElderGuardian;
impl ElderGuardian {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:elder_guardian";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:elder_guardian`
#[derive(Bundle, Clone)]
pub struct ElderGuardianBundle {
    pub attack: super::super::components::Attack,
    pub behavior_guardian_attack: super::super::components::BehaviorGuardianAttack,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_move_towards_home_restriction:
        super::super::components::BehaviorMoveTowardsHomeRestriction,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_swim: super::super::components::BehaviorRandomSwim,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_sway: super::super::components::MovementSway,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
    pub underwater_movement: super::super::components::UnderwaterMovement,
}
/// Spawn a new `minecraft:elder_guardian` entity with default Bedrock components
pub fn spawn_elder_guardian(commands: &mut Commands) -> Entity {
    commands
        .spawn(ElderGuardianBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(5f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
            },
            behavior_guardian_attack: super::super::components::BehaviorGuardianAttack {
                control_flags: Some(BehaviorGuardianAttackControlFlags {}),
                elder_extra_magic_damage: Some(2i32),
                hard_mode_extra_magic_damage: Some(2i32),
                magic_damage: Some(1i32),
                min_distance: Some(3f32),
                priority: Some(BehaviorGuardianAttackPriority {}),
                sound_delay_time: Some(0.5f32),
                x_max_rotation: Some(90f32),
                y_max_head_rotation: Some(90f32),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(12f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.01f32),
                target_distance: None,
            },
            behavior_move_towards_home_restriction:
                super::super::components::BehaviorMoveTowardsHomeRestriction {
                    priority: Some(BehaviorMoveTowardsHomeRestrictionPriority {}),
                    speed_multiplier: Some(BehaviorMoveTowardsHomeRestrictionSpeedMultiplier {}),
                },
            behavior_nearest_attackable_target:
                super::super::components::BehaviorNearestAttackableTarget {
                    attack_interval: Some(crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            ("max".to_string(), crate::types::BedrockValue::Integer(0i64)),
                            ("min".to_string(), crate::types::BedrockValue::Integer(0i64)),
                        ]),
                    )),
                    attack_interval_min: Some(1f32),
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
                                                    "axolotl".to_string(),
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
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: None,
                look_time: None,
                priority: Some(BehaviorRandomLookAroundPriority {}),
                probability: None,
            },
            behavior_random_swim: super::super::components::BehaviorRandomSwim {
                avoid_surface: Some(false),
                interval: Some(120i32),
                priority: Some(BehaviorRandomSwimPriority {}),
                speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {}),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            breathable: super::super::components::Breathable {
                breathe_blocks: None,
                breathes_air: Some(true),
                breathes_lava: Some(false),
                breathes_solids: Some(false),
                breathes_water: Some(true),
                can_dehydrate: Some(false),
                generates_bubbles: Some(true),
                inhale_time: Some(0f32),
                non_breathe_blocks: None,
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(1.99f32),
                width: Some(1.99f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            experience_reward: super::super::components::ExperienceReward {
                on_bred: Some(crate::types::MolangOr::Value(0f32)),
                on_death: Some(crate::types::MolangOr::Expr(
                    "query.last_hit_by_player ? 10 : 0".to_string(),
                )),
            },
            follow_range: super::super::components::FollowRange {
                max: Some(16f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            health: super::super::components::Health {
                max: Some(80f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(80f32),
            },
            home: super::super::components::Home {
                home_block_list: None,
                restriction_radius: Some(16i32),
                restriction_type: Some("random_movement".to_string()),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/elder_guardian.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
            movement_sway: super::super::components::MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0.05f32),
                sway_frequency: Some(0.5f32),
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
                can_path_over_water: Some(false),
                can_sink: Some(true),
                can_swim: Some(true),
                can_walk: Some(false),
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
                    "guardian_elder".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
            underwater_movement: super::super::components::UnderwaterMovement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.3f32),
            },
        })
        .id()
}
