//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:ominous_item_spawner`
pub struct OminousItemSpawner;
impl OminousItemSpawner {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ominous_item_spawner";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:ominous_item_spawner`
#[derive(Bundle, Clone)]
pub struct OminousItemSpawnerBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:ominous_item_spawner` entity with default Bedrock components
pub fn spawn_ominous_item_spawner(commands: &mut Commands) -> Entity {
    commands
        .spawn(OminousItemSpawnerBundle {
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            physics: Physics {
                has_gravity: true,
                has_collision: true,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
