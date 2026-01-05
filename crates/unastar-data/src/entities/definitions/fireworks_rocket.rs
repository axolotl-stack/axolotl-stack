//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:fireworks_rocket`
pub struct FireworksRocket;
impl FireworksRocket {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fireworks_rocket";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:fireworks_rocket`
#[derive(Bundle, Clone)]
pub struct FireworksRocketBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:fireworks_rocket` entity with default Bedrock components
pub fn spawn_fireworks_rocket(commands: &mut Commands) -> Entity {
    commands
        .spawn(FireworksRocketBundle {
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
