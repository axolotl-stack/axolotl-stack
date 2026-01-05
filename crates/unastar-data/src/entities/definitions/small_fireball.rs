//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:small_fireball`
pub struct SmallFireball;
impl SmallFireball {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:small_fireball";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:small_fireball`
#[derive(Bundle, Clone)]
pub struct SmallFireballBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:small_fireball` entity with default Bedrock components
pub fn spawn_small_fireball(commands: &mut Commands) -> Entity {
    commands
        .spawn(SmallFireballBundle {
            collision_box: CollisionBox {
                width: 0.31f32,
                height: 0.31f32,
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
