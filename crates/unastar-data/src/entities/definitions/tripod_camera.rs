//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
/// Entity definition for `minecraft:tripod_camera`
pub struct TripodCamera;
impl TripodCamera {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tripod_camera";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:tripod_camera`
#[derive(Bundle, Clone)]
pub struct TripodCameraBundle {
    pub collision_box: super::super::components::CollisionBox,
    pub conditional_bandwidth_optimization:
        super::super::components::ConditionalBandwidthOptimization,
    pub health: super::super::components::Health,
    pub hurt_on_condition: super::super::components::HurtOnCondition,
    pub loot: super::super::components::Loot,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
    pub type_family: super::super::components::TypeFamily,
}
/// Spawn a new `minecraft:tripod_camera` entity with default Bedrock components
pub fn spawn_tripod_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn(TripodCameraBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.75f32),
            },
            conditional_bandwidth_optimization:
                super::super::components::ConditionalBandwidthOptimization {
                    conditional_values: None,
                    default_values: None,
                },
            health: super::super::components::Health {
                max: Some(4f32),
                min: None,
                value: crate::types::RangeOrVal::Fixed(4f32),
            },
            hurt_on_condition: super::super::components::HurtOnCondition {
                damage_conditions: None,
            },
            loot: super::super::components::Loot {
                table: "loot_tables/empty.json".to_string(),
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
            type_family: super::super::components::TypeFamily {
                family: vec![
                    "tripodcamera".to_string(),
                    "inanimate".to_string(),
                    "mob".to_string(),
                ],
            },
        })
        .id()
}
