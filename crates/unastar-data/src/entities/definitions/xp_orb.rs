//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:xp_orb`
pub struct XpOrb;
impl XpOrb {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:xp_orb";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:xp_orb`
#[derive(Bundle, Clone)]
pub struct XpOrbBundle {
    pub buoyant: super::super::components::Buoyant,
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub health: super::super::components::Health,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:xp_orb` entity with default Bedrock components
pub fn spawn_xp_orb(commands: &mut Commands) -> Entity {
    commands
        .spawn(XpOrbBundle {
            buoyant: super::super::components::Buoyant {
                apply_gravity: Some(false),
                base_buoyancy: Some(1f32),
                big_wave_probability: Some(0.03f32),
                big_wave_speed: Some(10f32),
                buoyancy: None,
                drag_down_on_buoyancy_removed: Some(0f32),
                liquid_blocks: Some(vec![
                    crate::types::BedrockValue::String("minecraft:flowing_water".to_string()),
                    crate::types::BedrockValue::String("minecraft:water".to_string()),
                ]),
                simulate_waves: Some(true),
            },
            collision_box: super::super::components::CollisionBox {
                height: Some(0.25f32),
                width: Some(0.25f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: Some(ConditionalBandwidthOptimizationDefaultValues {
                        max_dropped_ticks: Some(10i32),
                        max_optimized_distance: Some(80f32),
                        use_motion_prediction_hints: Some(true),
                    }),
                },
            health: super::super::components::Health {
                max: Some(5f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(5f32),
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
                family: vec!["inanimate".to_string()],
            },
        })
        .id()
}
