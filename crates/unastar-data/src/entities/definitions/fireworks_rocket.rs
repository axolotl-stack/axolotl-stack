//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
                height: Some(0.25f32),
                width: Some(0.25f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
        })
        .id()
}
