//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:zombie_horse`
pub struct ZombieHorse;
impl ZombieHorse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_horse`
#[derive(Bundle, Clone)]
pub struct ZombieHorseBundle {
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_horse` entity with default Bedrock components
pub fn spawn_zombie_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieHorseBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            burns_in_daylight: BurnsInDaylight,
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 1.6f32,
            },
            health: Health {
                value: 25i32,
                max: Some(25i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.0 },
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
pub enum ZombieHorseComponentGroup {
    HorseAdult,
    HorseBaby,
    HorseCanBeLeashed,
    HorseSaddled,
    HorseTamed,
    HorseWild,
    HorseWildWithRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieHorseEvent {
    EntityBorn,
    EntitySpawned,
    HorseSaddled,
    HorseUnsaddled,
    HostileDismounted,
    HostileMounted,
    OnTame,
    SpawnAdult,
    SpawnAdultWithRider,
    SpawnTameAdult,
    UpgradeTo121130,
}
