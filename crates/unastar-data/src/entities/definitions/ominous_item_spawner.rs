//! Generated definition for entity.
#[allow(unused_imports)]
use super::super::components::*;
use bevy_ecs::prelude::{Bundle, Commands, Entity};
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
    pub collision_box: super::super::components::CollisionBox,
    pub physics: super::super::components::Physics,
    pub pushable: super::super::components::Pushable,
}
/// Spawn a new `minecraft:ominous_item_spawner` entity with default Bedrock components
pub fn spawn_ominous_item_spawner(commands: &mut Commands) -> Entity {
    commands
        .spawn(OminousItemSpawnerBundle {
            collision_box: super::super::components::CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
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
        })
        .id()
}
