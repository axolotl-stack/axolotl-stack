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
    pub dimension_bound: DimensionBound,
    pub loot: Loot,
    pub physics: Physics,
    pub pushable: Pushable,
    pub transient: Transient,
}
/// Spawn a new `minecraft:fishing_hook` entity with default Bedrock components
pub fn spawn_fishing_hook(commands: &mut Commands) -> Entity {
    commands
        .spawn(FishingHookBundle {
            collision_box: CollisionBox {
                height: Some(0.15f32),
                width: Some(0.15f32),
            },
            dimension_bound: DimensionBound,
            loot: Loot {
                table: "loot_tables/gameplay/fishing.json".to_string(),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(false),
                is_pushable_by_piston: Some(true),
            },
            transient: Transient,
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
