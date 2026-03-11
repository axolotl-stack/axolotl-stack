//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:parrot`
pub struct Parrot;
impl Parrot {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:parrot";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:parrot`
#[derive(Bundle, Clone)]
pub struct ParrotBundle {
    pub balloonable: super::super::components::Balloonable,
    pub behavior_float: super::super::components::BehaviorFloat,
    pub behavior_panic: super::super::components::BehaviorPanic,
    pub breathable: super::super::components::Breathable,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub game_event_movement_tracking: super::super::components::GameEventMovementTracking,
    pub healable: super::super::components::Healable,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub movement: super::super::components::Movement,
    pub movement_fly: super::super::components::MovementFly,
    pub nameable: super::super::components::Nameable,
    pub navigation_fly: super::super::components::NavigationFly,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:parrot` entity with default Bedrock components
pub fn spawn_parrot(commands: &mut Commands) -> Entity {
    commands
        .spawn(ParrotBundle {
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
                height: Some(1f32),
                width: Some(0.5f32),
            },
            conditional_bandwidth_optimization: super::super::components::ConditionalBandwidthOptimization {
                conditional_values: None,
                default_values: None,
            },
            damage_sensor: super::super::components::DamageSensor {
                triggers: Some(
                    vec![
                        DamageSensorTriggers { cause : Some("fall".to_string()),
                        damage_modifier : None, damage_multiplier : None, deals_damage :
                        Some("no".to_string()), on_damage : None, on_damage_sound_event :
                        None }
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
            game_event_movement_tracking: super::super::components::GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            healable: super::super::components::Healable {
                filters: Some(
                    crate::types::BedrockValue::Object(
                        std::collections::HashMap::from([
                            (
                                "operator".to_string(),
                                crate::types::BedrockValue::String("!=".to_string()),
                            ),
                            (
                                "test".to_string(),
                                crate::types::BedrockValue::String("is_riding".to_string()),
                            ),
                            ("value".to_string(), crate::types::BedrockValue::Bool(true)),
                        ]),
                    ),
                ),
                force_use: Some(true),
                items: Some(
                    vec![
                        HealableItems { effects : Some(vec![HealableItemsEffects {
                        amplifier : Some(0i32), duration : Some(crate
                        ::types::MolangOr::Value(1000i32)), name : Some("fatal_poison"
                        .to_string()) }]), filters : None, heal_amount : Some(0i32), item
                        : Some(crate ::types::BedrockValue::String("cookie"
                        .to_string())), result_item : None }
                    ],
                ),
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
                value: crate::types::RangeOrVal::Fixed(0.4f32),
            },
            movement_fly: super::super::components::MovementFly {
                max_turn: Some(30f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            navigation_fly: super::super::components::NavigationFly {
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
                can_path_from_air: Some(true),
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
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParrotComponentGroup {
    ParrotAdult,
    ParrotBlue,
    ParrotCyan,
    ParrotGreen,
    ParrotNotRidingPlayer,
    ParrotRed,
    ParrotRidingPlayer,
    ParrotSilver,
    ParrotTame,
    ParrotWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParrotEvent {
    EntitySpawned,
    OnNotRidingPlayer,
    OnRidingPlayer,
    OnTame,
}
