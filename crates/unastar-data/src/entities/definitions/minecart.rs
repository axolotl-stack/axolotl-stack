//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:minecart`
pub struct Minecart;
impl Minecart {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:minecart";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:minecart`
#[derive(Bundle, Clone)]
pub struct MinecartBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:minecart` entity with default Bedrock components
pub fn spawn_minecart(commands: &mut Commands) -> Entity {
    commands
        .spawn(MinecartBundle {
            collision_box: CollisionBox {
                width: 0.98f32,
                height: 0.7f32,
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
