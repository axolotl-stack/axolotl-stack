//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub collision_box: CollisionBox,
    pub health: Health,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:tripod_camera` entity with default Bedrock components
pub fn spawn_tripod_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn(TripodCameraBundle {
            collision_box: CollisionBox {
                width: 0.75f32,
                height: 1.8f32,
            },
            health: Health {
                value: 4i32,
                max: Some(4i32),
            },
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
