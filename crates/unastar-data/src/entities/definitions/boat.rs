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
    pub collision_box: CollisionBox,
    pub leashable: Leashable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:boat` entity with default Bedrock components
pub fn spawn_boat(commands: &mut Commands) -> Entity {
    commands
        .spawn(BoatBundle {
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 0.455f32,
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
