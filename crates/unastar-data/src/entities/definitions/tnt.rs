//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:tnt`
pub struct Tnt;
impl Tnt {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tnt";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tnt`
#[derive(Bundle, Clone)]
pub struct TntBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub explode: super::super::components::Explode,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:tnt` entity with default Bedrock components
pub fn spawn_tnt(commands: &mut Commands) -> Entity {
    commands
        .spawn(TntBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(0.98f32),
                width: Some(0.98f32),
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
            explode: super::super::components::Explode {
                allow_underwater: Some(false),
                breaks_blocks: Some(true),
                causes_fire: Some(false),
                damage_scaling: Some(1f32),
                destroy_affected_by_griefing: Some(false),
                fire_affected_by_griefing: Some(false),
                fuse_length: Some(crate::types::RangeOrVal::Fixed(4f32)),
                fuse_lit: Some(true),
                knockback_scaling: Some(1f32),
                max_resistance: Some(340282000000000000000000000000000000000f32),
                negates_fall_damage: Some(false),
                particle_effect: Some("explosion".to_string()),
                power: Some(4f32),
                sound_effect: Some("explode".to_string()),
                toggles_blocks: Some(false),
            },
            physics: super::super::components::Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: super::super::components::Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
            type_family: super::super::components::TypeFamily {
                family: vec!["tnt".to_string(), "inanimate".to_string()],
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TntComponentGroup {
    FromExplosion,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TntEvent {
    FromExplosion,
}
