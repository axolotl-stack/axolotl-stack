//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:drowned`
pub struct Drowned;
impl Drowned {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:drowned";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:drowned`
#[derive(Bundle, Clone)]
pub struct DrownedBundle {
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:drowned` entity with default Bedrock components
pub fn spawn_drowned(commands: &mut Commands) -> Entity {
    commands
        .spawn(DrownedBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
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
            movement: Movement { speed: 0.23f32 },
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
pub enum DrownedComponentGroup {
    AdultDrowned,
    BabyDrowned,
    CanBreakDoors,
    DrownedRider,
    HunterMode,
    MeleeEquipment,
    MeleeMode,
    ModeSwitcher,
    RangedEquipment,
    RangedMode,
    WanderMode,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrownedEvent {
    AsAdult,
    AsBaby,
    AsRangedAdult,
    AsRider,
    EntitySpawned,
    HasTarget,
    LostTarget,
    SwitchToMelee,
    SwitchToRanged,
}
