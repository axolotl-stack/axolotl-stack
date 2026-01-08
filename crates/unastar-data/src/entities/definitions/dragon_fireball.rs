//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:dragon_fireball`
pub struct DragonFireball;
impl DragonFireball {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:dragon_fireball";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:dragon_fireball`
#[derive(Bundle, Clone)]
pub struct DragonFireballBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:dragon_fireball` entity with default Bedrock components
pub fn spawn_dragon_fireball(commands: &mut Commands) -> Entity {
    commands
        .spawn(DragonFireballBundle {
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
