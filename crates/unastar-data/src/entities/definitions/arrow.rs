//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:arrow`
pub struct Arrow;
impl Arrow {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:arrow";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:arrow`
#[derive(Bundle, Clone)]
pub struct ArrowBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:arrow` entity with default Bedrock components
pub fn spawn_arrow(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArrowBundle {
            collision_box: CollisionBox {
                width: 0.25f32,
                height: 0.25f32,
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
pub enum ArrowComponentGroup {
    HardArrow,
    PillagerArrow,
    PlayerArrow,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrowEvent {
    EntitySpawned,
}
