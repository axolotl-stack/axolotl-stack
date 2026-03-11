//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:boat`
pub struct Boat;
impl Boat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:boat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:boat`
#[derive(Bundle, Clone)]
pub struct BoatBundle {
    pub balloonable: Balloonable,
    pub collision_box: CollisionBox,
    pub is_collidable: IsCollidable,
    pub is_stackable: IsStackable,
    pub leashable_to: LeashableTo,
    pub loot: Loot,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:boat` entity with default Bedrock components
pub fn spawn_boat(commands: &mut Commands) -> Entity {
    commands
        .spawn(BoatBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            collision_box: CollisionBox {
                height: Some(0.455f32),
                width: Some(1.4f32),
            },
            is_collidable: IsCollidable,
            is_stackable: IsStackable { value: false },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: Loot {
                table: "loot_tables/entities/boat.json".to_string(),
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoatComponentGroup {
    AboveBubbleColumnDown,
    AboveBubbleColumnUp,
    CanRideBamboo,
    CanRideDefault,
    Floating,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoatEvent {
    AddCanRide,
    EnteredBubbleColumnDown,
    EnteredBubbleColumnUp,
    EntitySpawned,
    ExitedBubbleColumn,
    Sink,
}
