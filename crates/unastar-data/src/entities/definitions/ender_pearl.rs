//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ender_pearl`
pub struct EnderPearl;
impl EnderPearl {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ender_pearl";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:ender_pearl`
#[derive(Bundle, Clone)]
pub struct EnderPearlBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ender_pearl` entity with default Bedrock components
pub fn spawn_ender_pearl(commands: &mut Commands) -> Entity {
    commands
        .spawn(EnderPearlBundle {
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderPearlComponentGroup {
    NoSpawn,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnderPearlEvent {
    EntitySpawned,
}
