//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
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
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub leashable: Leashable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:chest_boat` entity with default Bedrock components
pub fn spawn_chest_boat(commands: &mut Commands) -> Entity {
    commands
        .spawn(ChestBoatBundle {
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 0.455f32,
            },
            inventory: Inventory {
                size: 27i32,
                container_type: Some("chest_boat".to_string()),
                can_be_siphoned_from: true,
                private: false,
            },
            leashable: Leashable,
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
