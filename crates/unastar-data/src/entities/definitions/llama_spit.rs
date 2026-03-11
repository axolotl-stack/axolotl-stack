//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:llama_spit`
pub struct LlamaSpit;
impl LlamaSpit {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:llama_spit";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:llama_spit`
#[derive(Bundle, Clone)]
pub struct LlamaSpitBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:llama_spit` entity with default Bedrock components
pub fn spawn_llama_spit(commands: &mut Commands) -> Entity {
    commands
        .spawn(LlamaSpitBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.31f32),
                width: Some(0.31f32),
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
                gravity: Some(0.06f32),
                hit_ground_sound: None,
                hit_nearest_passenger: Some(false),
                hit_sound: None,
                homing: Some(false),
                ignored_entities: None,
                inertia: Some(1f32),
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
                        knockback: Some(false),
                        max_critical_damage: None,
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
                    remove_on_hit: Some(ProjectileOnHitRemoveOnHit {
                        additional: std::collections::HashMap::new(),
                    }),
                    spawn_aoe_cloud: None,
                    spawn_chance: None,
                    stick_in_ground: None,
                    teleport_owner: None,
                    thrown_potion_effect: None,
                }),
                particle: Some("iconcrack".to_string()),
                potion_effect: Some(-1i32),
                power: Some(1.5f32),
                reflect_immunity: Some(0f32),
                reflect_on_hurt: Some(true),
                semi_random_diff_damage: Some(false),
                shoot_sound: None,
                shoot_target: Some(true),
                should_bounce: Some(false),
                splash_potion: Some(false),
                splash_range: Some(4f32),
                stop_on_hurt: None,
                uncertainty_base: Some(10f32),
                uncertainty_multiplier: Some(4f32),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
