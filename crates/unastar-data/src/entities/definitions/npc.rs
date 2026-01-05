//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:npc`
pub struct Npc;
impl Npc {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:npc";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:npc`
#[derive(Bundle, Clone)]
pub struct NpcBundle {
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:npc` entity with default Bedrock components
pub fn spawn_npc(commands: &mut Commands) -> Entity {
    commands
        .spawn(NpcBundle {
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 2.1f32,
            },
            fire_immune: FireImmune,
            movement: Movement { speed: 0.5f32 },
            nameable: Nameable,
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
