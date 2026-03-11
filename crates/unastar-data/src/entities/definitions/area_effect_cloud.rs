//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:area_effect_cloud`
pub struct AreaEffectCloud;
impl AreaEffectCloud {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:area_effect_cloud";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:area_effect_cloud`
#[derive(Bundle, Clone)]
pub struct AreaEffectCloudBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:area_effect_cloud` entity with default Bedrock components
pub fn spawn_area_effect_cloud(commands: &mut Commands) -> Entity {
    commands
        .spawn(AreaEffectCloudBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            physics: super::super::components::Physics {
                has_collision: Some(false),
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
