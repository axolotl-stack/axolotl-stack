//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:wind_charge_projectile`
pub struct WindChargeProjectile;
impl WindChargeProjectile {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wind_charge_projectile";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wind_charge_projectile`
#[derive(Bundle, Clone)]
pub struct WindChargeProjectileBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub explode: super::super::components::Explode,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:wind_charge_projectile` entity with default Bedrock components
pub fn spawn_wind_charge_projectile(commands: &mut Commands) -> Entity {
    commands
        .spawn(WindChargeProjectileBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.3125f32),
                width: Some(0.3125f32),
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
            explode: super::super::components::Explode {
                allow_underwater: Some(true),
                breaks_blocks: Some(false),
                causes_fire: Some(false),
                damage_scaling: Some(0f32),
                destroy_affected_by_griefing: Some(false),
                fire_affected_by_griefing: Some(false),
                fuse_length: None,
                fuse_lit: Some(false),
                knockback_scaling: Some(1.22f32),
                max_resistance: Some(0f32),
                negates_fall_damage: Some(true),
                particle_effect: Some("wind_burst".to_string()),
                power: Some(1.2f32),
                sound_effect: Some("wind_charge.burst".to_string()),
                toggles_blocks: Some(true),
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            projectile: super::super::components::Projectile {
                anchor: None,
                angle_offset: Some(0f32),
                catch_fire: Some(false),
                crit_particle_on_hurt: Some(false),
                destroy_on_hurt: Some(false),
                filter: None,
                fire_affected_by_griefing: Some(false),
                gravity: Some(0f32),
                hit_ground_sound: None,
                hit_nearest_passenger: Some(false),
                hit_sound: None,
                homing: Some(false),
                ignored_entities: Some(vec![
                    "ender_crystal".to_string(),
                    "wind_charge_projectile".to_string(),
                    "breeze_wind_charge_projectile".to_string(),
                ]),
                inertia: Some(1f32),
                is_dangerous: Some(false),
                knockback: Some(true),
                lightning: Some(false),
                liquid_inertia: Some(1f32),
                mob_effect: None,
                multiple_targets: Some(false),
                offset: Some(vec![0f32, 0f32, 0f32]),
                on_fire_time: Some(5f32),
                on_hit: Some(ProjectileOnHit {
                    arrow_effect: None,
                    catch_fire: None,
                    definition_event: None,
                    douse_fire: None,
                    freeze_on_hit: None,
                    grant_xp: None,
                    hurt_owner: None,
                    ignite: None,
                    impact_damage: Some(ProjectileOnHitImpactDamage {
                        apply_knockback_to_blocking_targets: None,
                        catch_fire: None,
                        channeling: None,
                        damage: Some(crate::types::RangeOrVal::Fixed(1f32)),
                        destroy_on_hit: None,
                        destroy_on_hit_requires_damage: None,
                        filter: None,
                        knockback: Some(true),
                        max_critical_damage: Some(0i32),
                        min_critical_damage: None,
                        power_multiplier: None,
                        semi_random_diff_damage: None,
                        set_last_hurt_requires_damage: None,
                        should_bounce: None,
                    }),
                    mob_effect: None,
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
                power: Some(1.5f32),
                reflect_immunity: Some(0.5f32),
                reflect_on_hurt: Some(true),
                semi_random_diff_damage: Some(false),
                shoot_sound: None,
                shoot_target: Some(true),
                should_bounce: Some(false),
                splash_potion: Some(false),
                splash_range: Some(4f32),
                stop_on_hurt: None,
                uncertainty_base: Some(1f32),
                uncertainty_multiplier: Some(0f32),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "wind_charge".to_string(),
                    "wind_charge_projectile".to_string(),
                ],
            },
        })
        .id()
}
