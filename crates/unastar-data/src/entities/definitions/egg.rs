//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:egg`
pub struct Egg;
impl Egg {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:egg";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:egg`
#[derive(Bundle, Clone)]
pub struct EggBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:egg` entity with default Bedrock components
pub fn spawn_egg(commands: &mut Commands) -> Entity {
    commands
        .spawn(EggBundle {
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
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EggEvent {
    SpawnCold,
    SpawnTemperate,
    SpawnWarm,
}
