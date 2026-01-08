//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:parrot`
pub struct Parrot;
impl Parrot {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:parrot";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:parrot`
#[derive(Bundle, Clone)]
pub struct ParrotBundle {
    pub breathable: Breathable,
    pub can_fly: CanFly,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:parrot` entity with default Bedrock components
pub fn spawn_parrot(commands: &mut Commands) -> Entity {
    commands
        .spawn(ParrotBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_fly: CanFly,
            collision_box: CollisionBox {
                width: 0.5f32,
                height: 1f32,
            },
            health: Health {
                value: 6i32,
                max: Some(6i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.4f32 },
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
pub enum ParrotComponentGroup {
    ParrotAdult,
    ParrotBlue,
    ParrotCyan,
    ParrotGreen,
    ParrotNotRidingPlayer,
    ParrotRed,
    ParrotRidingPlayer,
    ParrotSilver,
    ParrotTame,
    ParrotWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParrotEvent {
    EntitySpawned,
    OnNotRidingPlayer,
    OnRidingPlayer,
    OnTame,
}
