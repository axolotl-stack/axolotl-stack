//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:fireball`
pub struct Fireball;
impl Fireball {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fireball";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:fireball`
#[derive(Bundle, Clone)]
pub struct FireballBundle {
    pub collision_box: CollisionBox,
    pub dimension_bound: DimensionBound,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:fireball` entity with default Bedrock components
pub fn spawn_fireball(commands: &mut Commands) -> Entity {
    commands
        .spawn(FireballBundle {
            collision_box: CollisionBox {
                height: Some(1f32),
                width: Some(1f32),
            },
            dimension_bound: DimensionBound,
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireballComponentGroup {
    Exploding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FireballEvent {
    Explode,
}
