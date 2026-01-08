//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:turtle`
pub struct Turtle;
impl Turtle {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:turtle";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:turtle`
#[derive(Bundle, Clone)]
pub struct TurtleBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:turtle` entity with default Bedrock components
pub fn spawn_turtle(commands: &mut Commands) -> Entity {
    commands
        .spawn(TurtleBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.8f32,
            },
            follow_range: FollowRange { range: 1024i32 },
            health: Health {
                value: 30i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.1f32 },
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleComponentGroup {
    Adult,
    Baby,
    Pregnant,
    WantsToLayEgg,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleEvent {
    AgeableGrowUp,
    BecomePregnant,
    EntityBorn,
    EntitySpawned,
    GoLayEgg,
    LaidEgg,
}
