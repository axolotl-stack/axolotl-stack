//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:ender_pearl`
pub struct EnderPearl;
impl EnderPearl {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ender_pearl";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:ender_pearl`
#[derive(Bundle, Clone)]
pub struct EnderPearlBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:ender_pearl` entity with default Bedrock components
pub fn spawn_ender_pearl(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderPearlBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.25f32),
                width: Some(0.25f32),
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
                anchor: None,
                angle_offset: Some(0f32),
                catch_fire: Some(false),
                crit_particle_on_hurt: Some(false),
                destroy_on_hurt: Some(false),
                filter: None,
                fire_affected_by_griefing: Some(false),
                gravity: Some(0.025f32),
                hit_ground_sound: None,
                hit_nearest_passenger: Some(false),
                hit_sound: None,
                homing: Some(false),
                ignored_entities: None,
                inertia: Some(1f32),
                is_dangerous: Some(false),
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
                    definition_event: None,
                    douse_fire: None,
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
                    spawn_aoe_cloud: None,
                    spawn_chance: Some(ProjectileOnHitSpawnChance {
                        first_spawn_chance: None,
                        first_spawn_count: Some(1i32),
                        first_spawn_percent_chance: Some(5f32),
                        on_spawn: None,
                        second_spawn_chance: None,
                        second_spawn_count: None,
                        spawn_baby: None,
                        spawn_definition: Some("minecraft:endermite".to_string()),
                    }),
                    stick_in_ground: None,
                    teleport_owner: Some(false),
                    thrown_potion_effect: None,
                }),
                particle: Some("iconcrack".to_string()),
                potion_effect: Some(-1i32),
                power: Some(1.5f32),
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderPearlComponentGroup {
    NoSpawn,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderPearlEvent {
    EntitySpawned,
}
