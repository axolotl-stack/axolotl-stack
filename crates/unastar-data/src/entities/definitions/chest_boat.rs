//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:chest_boat`
pub struct ChestBoat;
impl ChestBoat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:chest_boat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = false;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:chest_boat`
#[derive(Bundle, Clone)]
pub struct ChestBoatBundle {
    pub balloonable: Balloonable,
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub is_collidable: IsCollidable,
    pub is_stackable: IsStackable,
    pub leashable_to: LeashableTo,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:chest_boat` entity with default Bedrock components
pub fn spawn_chest_boat(commands: &mut Commands) -> Entity {
    commands
        .spawn(ChestBoatBundle {
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
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(true),
                container_type: Some("chest_boat".to_string()),
                inventory_size: Some(27i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_collidable: IsCollidable,
            is_stackable: IsStackable { value: false },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
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
pub enum ChestBoatComponentGroup {
    AboveBubbleColumnDown,
    AboveBubbleColumnUp,
    CanRideBamboo,
    CanRideDefault,
    Floating,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChestBoatEvent {
    AddCanRide,
    EnteredBubbleColumnDown,
    EnteredBubbleColumnUp,
    EntitySpawned,
    ExitedBubbleColumn,
    Sink,
}
