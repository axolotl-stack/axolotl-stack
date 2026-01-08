//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:wither_skull_dangerous`
pub struct WitherSkullDangerous;
impl WitherSkullDangerous {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wither_skull_dangerous";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:wither_skull_dangerous`
#[derive(Bundle, Clone)]
pub struct WitherSkullDangerousBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wither_skull_dangerous` entity with default Bedrock components
pub fn spawn_wither_skull_dangerous(commands: &mut Commands) -> Entity {
    commands
        .spawn(WitherSkullDangerousBundle {
            collision_box: CollisionBox {
                width: 0.15f32,
                height: 0.15f32,
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
pub enum WitherSkullDangerousComponentGroup {
    Exploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitherSkullDangerousEvent {
    Explode,
}
