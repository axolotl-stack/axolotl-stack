//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:bat`
pub struct Bat;
impl Bat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bat`
#[derive(Bundle, Clone)]
pub struct BatBundle {
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_float_wander: super::super::components::BehaviorFloatWander,
    pub breathable: super::super::components::Breathable,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub movement: super::super::components::Movement,
    pub movement_basic: super::super::components::MovementBasic,
    pub nameable: super::super::components::Nameable,
    pub navigation_float: super::super::components::NavigationFloat,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:bat` entity with default Bedrock components
pub fn spawn_bat(commands: &mut Commands) -> Entity {
    commands
        .spawn(BatBundle {
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
                float_wander_has_move_control: Some(true),
                must_reach: Some(false),
                navigate_around_surface: Some(false),
                priority: None,
                random_reselect: Some(true),
                surface_xz_dist: Some(0i32),
                surface_y_dist: Some(0i32),
                use_home_position_restriction: Some(true),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
                y_offset: Some(-2f32),
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
            collision_box: super::super::components::CollisionBox {
                height: Some(0.9f32),
                width: Some(0.5f32),
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
                    deals_damage: Some("false".to_string()),
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
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            health: super::super::components::Health {
                max: Some(6f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(6f32),
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
            movement_basic: super::super::components::MovementBasic {
                max_turn: Some(30f32),
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
            type_family: super::super::components::TypeFamily {
                family: vec!["bat".to_string(), "mob".to_string()],
            },
        })
        .id()
}
