//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:eye_of_ender_signal` entity with default Bedrock components
pub fn spawn_eye_of_ender_signal(commands: &mut Commands) -> Entity {
    commands
        .spawn(EyeOfEnderSignalBundle {
            collision_box: CollisionBox {
                width: 0.25f32,
                height: 0.25f32,
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
