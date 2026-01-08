//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:shulker_bullet`
pub struct ShulkerBullet;
impl ShulkerBullet {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:shulker_bullet";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:shulker_bullet`
#[derive(Bundle, Clone)]
pub struct ShulkerBulletBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:shulker_bullet` entity with default Bedrock components
pub fn spawn_shulker_bullet(commands: &mut Commands) -> Entity {
    commands
        .spawn(ShulkerBulletBundle {
            collision_box: CollisionBox {
                width: 0.625f32,
                height: 0.625f32,
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
