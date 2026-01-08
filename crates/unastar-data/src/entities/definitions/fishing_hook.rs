//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:fishing_hook`
pub struct FishingHook;
impl FishingHook {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fishing_hook";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = false;
}
/// Component bundle for spawning a `minecraft:fishing_hook`
#[derive(Bundle, Clone)]
pub struct FishingHookBundle {
    pub collision_box: CollisionBox,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:fishing_hook` entity with default Bedrock components
pub fn spawn_fishing_hook(commands: &mut Commands) -> Entity {
    commands
        .spawn(FishingHookBundle {
            collision_box: CollisionBox {
                width: 0.15f32,
                height: 0.15f32,
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FishingHookComponentGroup {
    LootJungle,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FishingHookEvent {
    EntitySpawned,
}
