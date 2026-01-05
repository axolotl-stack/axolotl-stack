//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:xp_bottle`
pub struct XpBottle;
impl XpBottle {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:xp_bottle";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:xp_bottle`
#[derive(Bundle, Clone)]
pub struct XpBottleBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:xp_bottle` entity with default Bedrock components
pub fn spawn_xp_bottle(commands: &mut Commands) -> Entity {
    commands
        .spawn(XpBottleBundle {
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
