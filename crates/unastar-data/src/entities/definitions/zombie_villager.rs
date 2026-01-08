//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie_villager`
pub struct ZombieVillager;
impl ZombieVillager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie_villager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie_villager`
#[derive(Bundle, Clone)]
pub struct ZombieVillagerBundle {
    pub attack: Attack,
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:zombie_villager` entity with default Bedrock components
pub fn spawn_zombie_villager(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieVillagerBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
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
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.9f32,
            },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum ZombieVillagerComponentGroup {
    Adult,
    Armorer,
    Baby,
    BecomeZombieVillagerV2,
    Butcher,
    CanBreakDoors,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    FromAbandonedVillage,
    Jockey,
    Leatherworker,
    Librarian,
    Shepherd,
    ToVillager,
    Toolsmith,
    Weaponsmith,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieVillagerEvent {
    FromVillage,
    BecomeCleric,
    EntitySpawned,
    EntityTransformed,
    VillagerConverted,
}
