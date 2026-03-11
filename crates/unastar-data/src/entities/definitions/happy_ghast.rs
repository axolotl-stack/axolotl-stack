//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:happy_ghast`
pub struct HappyGhast;
impl HappyGhast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:happy_ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:happy_ghast`
#[derive(Bundle, Clone)]
pub struct HappyGhastBundle {
    pub behavior_float: super::super::components::BehaviorFloat,
    pub body_rotation_always_follows_head: super::super::components::BodyRotationAlwaysFollowsHead,
    pub can_fly: super::super::components::CanFly,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub damage_sensor: super::super::components::DamageSensor,
    pub despawn: super::super::components::Despawn,
    pub follow_range: super::super::components::FollowRange,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
    pub is_tamed: super::super::components::IsTamed,
    pub jump_static: super::super::components::JumpStatic,
    pub leashable: super::super::components::Leashable,
    pub nameable: super::super::components::Nameable,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub renders_when_invisible: super::super::components::RendersWhenInvisible,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:happy_ghast` entity with default Bedrock components
pub fn spawn_happy_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(HappyGhastBundle {
            behavior_float: super::super::components::BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(BehaviorFloatPriority {}),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            body_rotation_always_follows_head:
                super::super::components::BodyRotationAlwaysFollowsHead,
            can_fly: super::super::components::CanFly {
                value: crate::types::BedrockValue::Null,
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(4f32),
                width: Some(4f32),
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
            follow_range: super::super::components::FollowRange {
                max: Some(16f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            is_hidden_when_invisible: super::super::components::IsHiddenWhenInvisible,
            is_tamed: super::super::components::IsTamed,
            jump_static: super::super::components::JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable: super::super::components::Leashable {
                can_be_cut: Some(true),
                can_be_stolen: Some(false),
                hard_distance: Some(6f32),
                max_distance: Some(0f32),
                on_leash: None,
                on_unleash: Some(LeashableOnUnleash {
                    event: Some("minecraft:on_unleashed".to_string()),
                    filters: None,
                    target: Some("self".to_string()),
                }),
                on_unleash_interact_only: Some(false),
                presets: Some(vec![LeashablePresets {
                    filter: None,
                    hard_distance: Some(10f32),
                    max_distance: Some(14f32),
                    rotation_adjustment: None,
                    soft_distance: None,
                    spring_type: None,
                }]),
                soft_distance: Some(4f32),
            },
            nameable: super::super::components::Nameable {
                allow_name_tag_renaming: Some(true),
                always_show: Some(false),
                default_trigger: None,
                name_actions: None,
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            renders_when_invisible: super::super::components::RendersWhenInvisible,
            type_family: super::super::components::TypeFamily {
                family: vec!["happy_ghast".to_string(), "mob".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HappyGhastComponentGroup {
    Adult,
    AdultHarnessed,
    AdultImmobile,
    AdultMobile,
    AdultUnharnessed,
    AdultWithPassengers,
    AdultWithoutPassengers,
    Baby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HappyGhastEvent {
    AgeableGrowUp,
    BecomeImmobile,
    BecomeMobile,
    EntitySpawned,
    OnHarnessed,
    OnPassengerDismount,
    OnPassengerMount,
    OnStopTempting,
    OnUnharnessed,
    OnUnleashed,
    SpawnAdult,
    SpawnBaby,
}
