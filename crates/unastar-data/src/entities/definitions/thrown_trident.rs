//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:thrown_trident`
pub struct ThrownTrident;
impl ThrownTrident {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:thrown_trident";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:thrown_trident`
#[derive(Bundle, Clone)]
pub struct ThrownTridentBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:thrown_trident` entity with default Bedrock components
pub fn spawn_thrown_trident(commands: &mut Commands) -> Entity {
    commands
        .spawn(ThrownTridentBundle {
            collision_box: CollisionBox {
                width: 0.25f32,
                height: 0.35f32,
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
