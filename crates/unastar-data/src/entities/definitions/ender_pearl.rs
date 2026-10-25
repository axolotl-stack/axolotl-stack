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
    pub attack: super::super::components::Attack,
    pub breathable: super::super::components::Breathable,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub flying_speed: super::super::components::FlyingSpeed,
    pub follow_range: super::super::components::FollowRange,
    pub health: super::super::components::Health,
    pub inventory: super::super::components::Inventory,
    pub movement: super::super::components::Movement,
    pub physics: super::super::components::Physics,
    pub projectile: super::super::components::Projectile,
    pub pushable: super::super::components::Pushable,
    pub pushable_by_block: super::super::components::PushableByBlock,
    pub pushable_by_entity: super::super::components::PushableByEntity,
    pub scale: super::super::components::Scale,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:ender_pearl` entity with default Bedrock components
pub fn spawn_ender_pearl(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderPearlBundle {
            attack: super::super::components::Attack {
                damage: crate::types::RangeOrVal::Fixed(2f32),
                effect_duration: Some(crate::types::MolangOr::Value(0i32)),
                effect_name: None,
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
                suffocate_time: Some(-1i32),
                total_supply: Some(15i32),
            },
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
            flying_speed: super::super::components::FlyingSpeed { value: 0.02f32 },
            follow_range: super::super::components::FollowRange {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(16f32),
            },
            health: super::super::components::Health {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(20f32),
            },
            inventory: super::super::components::Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(5i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            movement: super::super::components::Movement {
                max: None,
                min: None,
                value: crate::types::RangeOrVal::Fixed(0.25f32),
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
                        first_spawn_chance: Some(0.05f32),
                        first_spawn_count: Some(1i32),
                        first_spawn_percent_chance: None,
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
            pushable_by_block: super::super::components::PushableByBlock { data: None },
            pushable_by_entity: super::super::components::PushableByEntity { data: None },
            scale: super::super::components::Scale { value: 1f32 },
            type_family: super::super::components::TypeFamily {
                family: vec!["projectile".to_string(), "ender_pearl".to_string()],
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
