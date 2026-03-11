//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
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
    pub loot: Loot,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:npc` entity with default Bedrock components
pub fn spawn_npc(commands: &mut Commands) -> Entity {
    commands
        .spawn(NpcBundle {
            collision_box: CollisionBox {
                height: Some(2.1f32),
                width: Some(0.6f32),
            },
            fire_immune: FireImmune,
            loot: Loot {
                table: "loot_tables/empty.json".to_string(),
            },
            persistent: Persistent,
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
