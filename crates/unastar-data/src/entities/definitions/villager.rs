//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:villager`
pub struct Villager;
impl Villager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:villager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:villager`
#[derive(Bundle, Clone)]
pub struct VillagerBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:villager` entity with default Bedrock components
pub fn spawn_villager(commands: &mut Commands) -> Entity {
    commands
        .spawn(VillagerBundle {
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
                width: 0.6f32,
                height: 1.9f32,
            },
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            inventory: Inventory {
                size: 8i32,
                container_type: None,
                can_be_siphoned_from: false,
                private: true,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.5f32 },
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
pub enum VillagerComponentGroup {
    Adult,
    Armorer,
    Baby,
    BecomeVillagerV2,
    BecomeWitch,
    BecomeZombie,
    BehaviorNonPeasant,
    BehaviorPeasant,
    Butcher,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    Leatherworker,
    Librarian,
    Celebrate,
    Shepherd,
    Toolsmith,
    Weaponsmith,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VillagerEvent {
    BecomeWitch,
    BecomeZombie,
    AgeableGrowUp,
    BecomeCleric,
    EntityBorn,
    EntitySpawned,
    EntityTransformed,
    SpawnArmorer,
    SpawnButcher,
    SpawnCleric,
    SpawnFarmer,
    SpawnLibrarian,
    StartCelebrating,
    StopCelebrating,
}
