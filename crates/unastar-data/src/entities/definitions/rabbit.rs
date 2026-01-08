//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:rabbit`
pub struct Rabbit;
impl Rabbit {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:rabbit";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:rabbit`
#[derive(Bundle, Clone)]
pub struct RabbitBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:rabbit` entity with default Bedrock components
pub fn spawn_rabbit(commands: &mut Commands) -> Entity {
    commands
        .spawn(RabbitBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.67f32,
                height: 0.67f32,
            },
            health: Health {
                value: 3i32,
                max: Some(3i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.3f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: true,
                is_pushable_by_piston: true,
            },
            scale: Scale { value: 0.6f32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitComponentGroup {
    Adult,
    Baby,
    CoatBlack,
    CoatBrown,
    CoatDesert,
    CoatSalt,
    CoatSplotched,
    CoatWhite,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitEvent {
    GrowUp,
    InDesert,
    InSnow,
    EntityBorn,
    EntitySpawned,
}
