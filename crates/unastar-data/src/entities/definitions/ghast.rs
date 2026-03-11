//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:ghast`
pub struct Ghast;
impl Ghast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ghast`
#[derive(Bundle, Clone)]
pub struct GhastBundle {
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_float_wander: super::super::components::BehaviorFloatWander,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_nearest_attackable_target:
        super::super::components::BehaviorNearestAttackableTarget,
    pub behavior_ranged_attack: super::super::components::BehaviorRangedAttack,
    pub breathable: super::super::components::Breathable,
    pub can_fly: super::super::components::CanFly,
    pub cannot_be_attacked: super::super::components::CannotBeAttacked,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub fire_immune: super::super::components::FireImmune,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub nameable: super::super::components::Nameable,
    pub navigation_float: super::super::components::NavigationFloat,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub shooter: super::super::components::Shooter,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:ghast` entity with default Bedrock components
pub fn spawn_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(GhastBundle {
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_float_wander: super::super::components::BehaviorFloatWander {
                additional_collision_buffer: Some(false),
                allow_navigating_through_liquids: Some(false),
                float_duration: None,
                float_wander_has_move_control: Some(false),
                must_reach: Some(true),
                navigate_around_surface: Some(false),
                priority: Some(BehaviorFloatWanderPriority {}),
                random_reselect: Some(true),
                surface_xz_dist: Some(0i32),
                surface_y_dist: Some(0i32),
                use_home_position_restriction: Some(true),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
                y_offset: Some(0f32),
            },
            behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget {
                alert_same_type: Some(false),
                entity_types: None,
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
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
                                    crate::types::BedrockValue::String("player".to_string()),
                                ),
                            ]),
                        )),
                        max_dist: Some(28f32),
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
            behavior_ranged_attack: super::super::components::BehaviorRangedAttack {
                attack_interval: Some(0f32),
                attack_interval_max: Some(0f32),
                attack_interval_min: Some(0f32),
                attack_radius: Some(64f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(1f32),
                charge_shoot_trigger: Some(2f32),
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
            can_fly: super::super::components::CanFly {
                value: crate::types::BedrockValue::Null,
            },
            cannot_be_attacked: super::super::components::CannotBeAttacked,
            collision_box: super::super::components::CollisionBox {
                height: Some(4f32),
                width: Some(4.02f32),
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
                    "query.last_hit_by_player ? 5 + (query.equipment_count * Math.Random(1,3)) : 0"
                        .to_string(),
                )),
            },
            fire_immune: super::super::components::FireImmune,
            follow_range: super::super::components::FollowRange {
                max: Some(64f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(10f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(10f32),
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/ghast.json".to_string(),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.03f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_float: super::super::components::NavigationFloat {
                avoid_damage_blocks: Some(false),
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
            shooter: super::super::components::Shooter {
                aux_val: Some(-1i32),
                def: Some("minecraft:fireball".to_string()),
                magic: Some(false),
                power: Some(0f32),
                projectiles: None,
                sound: None,
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "ghast".to_string(),
                    "monster".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
