//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:tnt` entity with default Bedrock components
pub fn spawn_tnt(commands: &mut Commands) -> Entity {
    commands
        .spawn(TntBundle {
            collision_box: CollisionBox {
                width: 0.98f32,
                height: 0.98f32,
            },
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: true,
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
