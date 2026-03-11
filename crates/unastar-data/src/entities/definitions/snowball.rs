//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:snowball`
pub struct Snowball;
impl Snowball {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:snowball";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:snowball`
#[derive(Bundle, Clone)]
pub struct SnowballBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:snowball` entity with default Bedrock components
pub fn spawn_snowball(commands: &mut Commands) -> Entity {
    commands
        .spawn(SnowballBundle {
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
