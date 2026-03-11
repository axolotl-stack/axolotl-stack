//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:witch`
pub struct Witch;
impl Witch {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:witch";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:witch`
#[derive(Bundle, Clone)]
pub struct WitchBundle {
    pub behavior_drink_potion: super::super::components::BehaviorDrinkPotion,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_nearest_prioritized_attackable_target:
        super::super::components::BehaviorNearestPrioritizedAttackableTarget,
    pub behavior_random_look_around: super::super::components::BehaviorRandomLookAround,
    pub behavior_random_stroll: super::super::components::BehaviorRandomStroll,
    pub behavior_ranged_attack: super::super::components::BehaviorRangedAttack,
    pub breathable: super::super::components::Breathable,
    pub can_climb: super::super::components::CanClimb,
    pub can_join_raid: super::super::components::CanJoinRaid,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub experience_reward: super::super::components::ExperienceReward,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub loot: super::super::components::Loot,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_walk: super::super::components::NavigationWalk,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub shooter: super::super::components::Shooter,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:witch` entity with default Bedrock components
pub fn spawn_witch(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitchBundle {
            behavior_drink_potion: super::super::components::BehaviorDrinkPotion {
                potions: Some(
                    vec![
                        BehaviorDrinkPotionPotions { chance : 0.15f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_underwater".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("water_breathing"
                        .to_string()))]))]))]))]))])), id : 19i32 },
                        BehaviorDrinkPotionPotions { chance : 0.15f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("on_fire".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("on_hot_block".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("taking_fire_damage".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))]))]))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("fire_resistance"
                        .to_string()))]))]))]))]))])), id : 12i32 },
                        BehaviorDrinkPotionPotions { chance : 0.05f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_missing_health".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))]))]))])), id : 21i32 },
                        BehaviorDrinkPotionPotions { chance : 0.25f32, filters : crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_target".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate ::types::BedrockValue::String("speed"
                        .to_string()))]))]))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String(">="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("self".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("target_distance".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Float(11f64))]))]))])), id : 14i32 }
                    ],
                ),
                priority: Some(BehaviorDrinkPotionPriority {}),
                speed_modifier: Some(crate::types::BedrockValue::Float(-0.25f64)),
                speed_multiplier: None,
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
                hurt_owner: Some(false),
                priority: Some(BehaviorHurtByTargetPriority {}),
            },
            behavior_look_at_player: super::super::components::BehaviorLookAtPlayer {
                angle_of_view_horizontal: Some(360i32),
                angle_of_view_vertical: Some(360i32),
                look_distance: Some(16f32),
                look_time: None,
                priority: Some(BehaviorLookAtPlayerPriority {}),
                probability: Some(0.02f32),
                target_distance: None,
            },
            behavior_nearest_prioritized_attackable_target: super::super::components::BehaviorNearestPrioritizedAttackableTarget {
                attack_interval: Some(0i32),
                cooldown: Some(0f32),
                entity_types: Some(
                    vec![
                        BehaviorNearestPrioritizedAttackableTargetEntityTypes {
                        check_if_outnumbered : None, cooldown : None, filters :
                        Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("any_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("player"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("snowgolem"
                        .to_string()))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("irongolem"
                        .to_string()))]))]))]))), max_dist : Some(10f32), max_flee :
                        None, max_height : None, must_see : None,
                        must_see_forget_duration : None, priority : Some(1f32),
                        reevaluate_description : None, sprint_speed_multiplier : None,
                        walk_speed_multiplier : None, within_default : None },
                        BehaviorNearestPrioritizedAttackableTargetEntityTypes {
                        check_if_outnumbered : None, cooldown : Some(10f32), filters :
                        Some(crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_raider".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("self"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_raider".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_family".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::String("witch"
                        .to_string()))]))]))]))]))]))), max_dist : Some(10f32), max_flee
                        : None, max_height : None, must_see : None,
                        must_see_forget_duration : None, priority : Some(2f32),
                        reevaluate_description : None, sprint_speed_multiplier : None,
                        walk_speed_multiplier : None, within_default : None }
                    ],
                ),
                must_reach: Some(true),
                must_see: Some(false),
                must_see_forget_duration: Some(3f32),
                persist_time: Some(0f32),
                priority: Some(BehaviorNearestPrioritizedAttackableTargetPriority {
                }),
                reevaluate_description: None,
                reselect_targets: Some(false),
                scan_interval: Some(10i32),
                set_persistent: Some(false),
                target_search_height: Some(-1f32),
                within_radius: Some(0f32),
            },
            behavior_random_look_around: super::super::components::BehaviorRandomLookAround {
                angle_of_view_horizontal: None,
                angle_of_view_vertical: None,
                look_distance: Some(8f32),
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
            behavior_ranged_attack: super::super::components::BehaviorRangedAttack {
                attack_interval: Some(0f32),
                attack_interval_max: Some(3f32),
                attack_interval_min: Some(3f32),
                attack_radius: Some(10f32),
                attack_radius_min: Some(0f32),
                burst_interval: Some(0f32),
                burst_shots: Some(1i32),
                charge_charged_trigger: Some(0f32),
                charge_shoot_trigger: Some(0f32),
                priority: Some(BehaviorRangedAttackPriority {}),
                ranged_fov: Some(90f32),
                set_persistent: Some(false),
                speed_multiplier: Some(BehaviorRangedAttackSpeedMultiplier {
                }),
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
            can_join_raid: super::super::components::CanJoinRaid,
            collision_box: super::super::components::CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: None,
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
                on_death: Some(
                    crate::types::MolangOr::Expr(
                        "query.last_hit_by_player ? (query.is_baby ? 12 : 5) + (Math.die_roll(query.equipment_count,1,3)) : 0"
                            .to_string(),
                    ),
                ),
            },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(64f32),
            },
            health: super::super::components::Health {
                max: Some(26f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(26f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            loot: super::super::components::Loot {
                table: "loot_tables/entities/witch.json".to_string(),
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
                avoid_damage_blocks: Some(false),
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
                aux_val: Some(23i32),
                def: Some("minecraft:splash_potion".to_string()),
                magic: Some(true),
                power: Some(0.75f32),
                projectiles: Some(
                    vec![
                        crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("aux_val"
                        .to_string(), crate ::types::BedrockValue::Integer(21i64)),
                        ("def".to_string(), crate
                        ::types::BedrockValue::String("minecraft:splash_potion"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_raider".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("<="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("actor_health"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Integer(4i64))]))]))]))), ("lose_target"
                        .to_string(), crate ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("aux_val"
                        .to_string(), crate ::types::BedrockValue::Integer(28i64)),
                        ("def".to_string(), crate
                        ::types::BedrockValue::String("minecraft:splash_potion"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("is_raider".to_string())), ("value"
                        .to_string(), crate ::types::BedrockValue::Bool(true))]))]))]))),
                        ("lose_target".to_string(), crate
                        ::types::BedrockValue::Bool(true))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("aux_val"
                        .to_string(), crate ::types::BedrockValue::Integer(17i64)),
                        ("def".to_string(), crate
                        ::types::BedrockValue::String("minecraft:splash_potion"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String(">="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("self".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("target_distance".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Float(8f64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("slowness"
                        .to_string()))]))]))]))]))])))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("aux_val"
                        .to_string(), crate ::types::BedrockValue::Integer(25i64)),
                        ("def".to_string(), crate
                        ::types::BedrockValue::String("minecraft:splash_potion"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String(">="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("other".to_string())), ("test"
                        .to_string(), crate ::types::BedrockValue::String("actor_health"
                        .to_string())), ("value".to_string(), crate
                        ::types::BedrockValue::Integer(8i64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("poison"
                        .to_string()))]))]))]))]))])))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("aux_val"
                        .to_string(), crate ::types::BedrockValue::Integer(34i64)),
                        ("chance".to_string(), crate
                        ::types::BedrockValue::Float(0.25f64)), ("def".to_string(), crate
                        ::types::BedrockValue::String("minecraft:splash_potion"
                        .to_string())), ("filters".to_string(), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("all_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("operator"
                        .to_string(), crate ::types::BedrockValue::String("<="
                        .to_string())), ("subject".to_string(), crate
                        ::types::BedrockValue::String("self".to_string())), ("test"
                        .to_string(), crate
                        ::types::BedrockValue::String("target_distance".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::Integer(3i64))])), crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("none_of"
                        .to_string(), crate ::types::BedrockValue::Array(vec![crate
                        ::types::BedrockValue::Object(std::collections::HashMap::from([("subject"
                        .to_string(), crate ::types::BedrockValue::String("other"
                        .to_string())), ("test".to_string(), crate
                        ::types::BedrockValue::String("has_mob_effect".to_string())),
                        ("value".to_string(), crate
                        ::types::BedrockValue::String("weakness"
                        .to_string()))]))]))]))]))])))]))
                    ],
                ),
                sound: Some("throw".to_string()),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "witch".to_string(), "monster".to_string(), "mob".to_string()
                ],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitchComponentGroup {
    Celebrate,
    RaidConfiguration,
    RaidPersistence,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitchEvent {
    RaidExpired,
    SpawnForRaid,
    StartCelebrating,
    StopCelebrating,
}
