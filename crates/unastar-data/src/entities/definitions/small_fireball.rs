//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub dimension_bound: DimensionBound,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:small_fireball` entity with default Bedrock components
pub fn spawn_small_fireball(commands: &mut Commands) -> Entity {
    commands
        .spawn(SmallFireballBundle {
            collision_box: CollisionBox {
                height: Some(0.31f32),
                width: Some(0.31f32),
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
