//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:eye_of_ender_signal`
pub struct EyeOfEnderSignal;
impl EyeOfEnderSignal {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:eye_of_ender_signal";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:eye_of_ender_signal`
#[derive(Bundle, Clone)]
pub struct EyeOfEnderSignalBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:eye_of_ender_signal` entity with default Bedrock components
pub fn spawn_eye_of_ender_signal(commands: &mut Commands) -> Entity {
    commands
        .spawn(EyeOfEnderSignalBundle {
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
