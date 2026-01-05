//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:fireball`
pub struct Fireball;
impl Fireball {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fireball";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:fireball`
#[derive(Bundle, Clone)]
pub struct FireballBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:fireball` entity with default Bedrock components
pub fn spawn_fireball(commands: &mut Commands) -> Entity {
    commands
        .spawn(FireballBundle {
            collision_box: CollisionBox {
                width: 1f32,
                height: 1f32,
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireballComponentGroup {
    Exploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireballEvent {
    Explode,
}
