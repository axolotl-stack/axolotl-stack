//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:wither_skull_dangerous`
pub struct WitherSkullDangerous;
impl WitherSkullDangerous {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wither_skull_dangerous";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:wither_skull_dangerous`
#[derive(Bundle, Clone)]
pub struct WitherSkullDangerousBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub dimension_bound: super::super::components::DimensionBound,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:wither_skull_dangerous` entity with default Bedrock components
pub fn spawn_wither_skull_dangerous(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitherSkullDangerousBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.15f32),
                width: Some(0.15f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(7i32),
                        max_optimized_distance: Some(80f32),
                        use_motion_prediction_hints: Some(true),
                    }),
                },
            dimension_bound: super::super::components::DimensionBound,
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            projectile: super::super::components::Projectile {
                anchor: Some(1i32),
                angle_offset: Some(0f32),
                catch_fire: Some(false),
                crit_particle_on_hurt: Some(false),
                destroy_on_hurt: Some(false),
                filter: None,
                fire_affected_by_griefing: Some(false),
                gravity: Some(0f32),
                hit_ground_sound: None,
                hit_nearest_passenger: Some(false),
                hit_sound: Some("bow.hit".to_string()),
                homing: Some(false),
                ignored_entities: None,
                inertia: Some(1f32),
                is_dangerous: Some(true),
                knockback: Some(true),
                lightning: Some(false),
                liquid_inertia: Some(1f32),
                mob_effect: None,
                multiple_targets: Some(true),
                offset: Some(vec![0f32, 0f32, 0f32]),
                on_fire_time: Some(5f32),
                on_hit: Some(ProjectileOnHit {
                    arrow_effect: None,
                    catch_fire: None,
                    definition_event: Some(ProjectileOnHitDefinitionEvent {
                        affect_projectile: Some(true),
                        affect_shooter: None,
                        affect_splash_area: None,
                        affect_target: None,
                        event_trigger: Some(crate::types::BedrockValue::Object(
                            std::collections::HashMap::from([
                                (
                                    "event".to_string(),
                                    crate::types::BedrockValue::String(
                                        "minecraft:explode".to_string(),
                                    ),
                                ),
                                (
                                    "target".to_string(),
                                    crate::types::BedrockValue::String("self".to_string()),
                                ),
                            ]),
                        )),
                        splash_area: None,
                    }),
                    douse_fire: None,
                    freeze_on_hit: None,
                    grant_xp: None,
                    hurt_owner: None,
                    ignite: None,
                    impact_damage: None,
                    mob_effect: Some(ProjectileOnHitMobEffect {
                        ambient: None,
                        amplifier: Some(1i32),
                        duration: None,
                        durationeasy: Some(crate::types::MolangOr::Value(0i32)),
                        durationhard: Some(crate::types::MolangOr::Value(800i32)),
                        durationnormal: Some(crate::types::MolangOr::Value(200i32)),
                        effect: Some("wither".to_string()),
                        visible: None,
                    }),
                    on_fire_time: None,
                    particle_on_hit: None,
                    potion_effect: None,
                    remove_on_hit: None,
                    spawn_aoe_cloud: None,
                    spawn_chance: None,
                    stick_in_ground: None,
                    teleport_owner: None,
                    thrown_potion_effect: None,
                }),
                particle: Some("iconcrack".to_string()),
                potion_effect: Some(-1i32),
                power: Some(0.6f32),
                reflect_immunity: Some(0f32),
                reflect_on_hurt: Some(true),
                semi_random_diff_damage: Some(false),
                shoot_sound: Some("bow".to_string()),
                shoot_target: Some(false),
                should_bounce: Some(false),
                splash_potion: Some(false),
                splash_range: Some(4f32),
                stop_on_hurt: None,
                uncertainty_base: Some(7.5f32),
                uncertainty_multiplier: Some(1f32),
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
pub enum WitherSkullDangerousComponentGroup {
    Exploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitherSkullDangerousEvent {
    Explode,
}
