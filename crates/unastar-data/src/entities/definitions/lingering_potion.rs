//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:lingering_potion`
pub struct LingeringPotion;
impl LingeringPotion {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:lingering_potion";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:lingering_potion`
#[derive(Bundle, Clone)]
pub struct LingeringPotionBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:lingering_potion` entity with default Bedrock components
pub fn spawn_lingering_potion(commands: &mut Commands) -> Entity {
    commands
        .spawn(LingeringPotionBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.25f32),
                width: Some(0.25f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(5i32),
                        max_optimized_distance: Some(80f32),
                        use_motion_prediction_hints: Some(true),
                    }),
                },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            projectile: super::super::components::Projectile {
                anchor: None,
                angle_offset: Some(-20f32),
                catch_fire: Some(false),
                crit_particle_on_hurt: Some(false),
                destroy_on_hurt: Some(false),
                filter: None,
                fire_affected_by_griefing: Some(false),
                gravity: Some(0.05f32),
                hit_ground_sound: None,
                hit_nearest_passenger: Some(false),
                hit_sound: Some("glass".to_string()),
                homing: Some(false),
                ignored_entities: None,
                inertia: Some(0.99f32),
                is_dangerous: Some(false),
                knockback: Some(true),
                lightning: Some(false),
                liquid_inertia: Some(0.6f32),
                mob_effect: None,
                multiple_targets: Some(true),
                offset: Some(vec![0f32, 0f32, 0f32]),
                on_fire_time: Some(5f32),
                on_hit: Some(ProjectileOnHit {
                    arrow_effect: None,
                    catch_fire: None,
                    definition_event: None,
                    douse_fire: Some(false),
                    freeze_on_hit: None,
                    grant_xp: None,
                    hurt_owner: None,
                    ignite: None,
                    impact_damage: None,
                    mob_effect: None,
                    on_fire_time: None,
                    particle_on_hit: None,
                    potion_effect: None,
                    remove_on_hit: Some(ProjectileOnHitRemoveOnHit {
                        additional: std::collections::HashMap::new(),
                    }),
                    spawn_aoe_cloud: Some(ProjectileOnHitSpawnAoeCloud {
                        affect_owner: None,
                        color: None,
                        duration: Some(30i32),
                        particle: None,
                        potion: None,
                        radius: Some(3f32),
                        radius_on_use: Some(-0.5f32),
                        reapplication_delay: Some(40i32),
                    }),
                    spawn_chance: None,
                    stick_in_ground: None,
                    teleport_owner: None,
                    thrown_potion_effect: None,
                }),
                particle: Some("iconcrack".to_string()),
                potion_effect: Some(-1i32),
                power: Some(0.5f32),
                reflect_immunity: Some(0f32),
                reflect_on_hurt: Some(false),
                semi_random_diff_damage: Some(false),
                shoot_sound: None,
                shoot_target: Some(true),
                should_bounce: Some(false),
                splash_potion: Some(false),
                splash_range: Some(4f32),
                stop_on_hurt: None,
                uncertainty_base: Some(0f32),
                uncertainty_multiplier: Some(0f32),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
