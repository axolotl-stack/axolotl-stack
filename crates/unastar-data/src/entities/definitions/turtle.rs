//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:turtle`
pub struct Turtle;
impl Turtle {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:turtle";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:turtle`
#[derive(Bundle, Clone)]
pub struct TurtleBundle {
    pub behavior_hurt_by_target: super::super::components::BehaviorHurtByTarget,
    pub behavior_look_at_player: super::super::components::BehaviorLookAtPlayer,
    pub behavior_move_to_water: super::super::components::BehaviorMoveToWater,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub behavior_random_swim: super::super::components::BehaviorRandomSwim,
    pub behavior_tempt: super::super::components::BehaviorTempt,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub home: super::super::components::Home,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_amphibious: super::super::components::MovementAmphibious,
    pub nameable: super::super::components::Nameable,
    pub navigation_generic: super::super::components::NavigationGeneric,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub water_movement: super::super::components::WaterMovement,
}
/// Spawn a new `minecraft:turtle` entity with default Bedrock components
pub fn spawn_turtle(commands: &mut Commands) -> Entity {
    commands
        .spawn(TurtleBundle {
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
                target_distance: Some(6f32),
            },
            behavior_move_to_water: super::super::components::BehaviorMoveToWater {
                goal_radius: Some(1.5f32),
                priority: Some(BehaviorMoveToWaterPriority {}),
                search_count: Some(10i32),
                search_height: Some(5i32),
                search_range: Some(16i32),
                speed_multiplier: Some(BehaviorMoveToWaterSpeedMultiplier {
                }),
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
                prefer_water: Some(true),
                priority: Some(BehaviorPanicPriority {}),
                sound_interval: None,
                speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
            },
            behavior_random_swim: super::super::components::BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(BehaviorRandomSwimPriority {}),
                speed_multiplier: Some(BehaviorRandomSwimSpeedMultiplier {
                }),
                xz_dist: Some(30i32),
                y_dist: Some(15i32),
            },
            behavior_tempt: super::super::components::BehaviorTempt {
                can_get_scared: Some(false),
                can_tempt_vertically: Some(true),
                can_tempt_while_ridden: Some(false),
                items: Some(
                    vec![crate ::types::BedrockValue::String("seagrass".to_string())],
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
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : Some("lightning".to_string()),
                        damage_modifier : None, damage_multiplier : Some(2000f32),
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
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(1024f32),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(30f32),
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
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.1f32),
            },
            movement_amphibious: super::super::components::MovementAmphibious {
                max_turn: Some(5f32),
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
                can_path_over_water: Some(false),
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
            water_movement: super::super::components::WaterMovement {
                drag_factor: Some(0.9f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleComponentGroup {
    Adult,
    Baby,
    Pregnant,
    WantsToLayEgg,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleEvent {
    AgeableGrowUp,
    BecomePregnant,
    EntityBorn,
    EntitySpawned,
    GoLayEgg,
    LaidEgg,
}
