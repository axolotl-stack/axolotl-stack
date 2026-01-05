//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:xp_orb`
pub struct XpOrb;
impl XpOrb {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:xp_orb";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:xp_orb`
#[derive(Bundle, Clone)]
pub struct XpOrbBundle {
    pub collision_box: CollisionBox,
    pub health: Health,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:xp_orb` entity with default Bedrock components
pub fn spawn_xp_orb(commands: &mut Commands) -> Entity {
    commands
        .spawn(XpOrbBundle {
            collision_box: CollisionBox {
                width: 0.25f32,
                height: 0.25f32,
            },
            health: Health {
                value: 5i32,
                max: Some(5i32),
            },
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: true,
            },
        })
        .id()
}
