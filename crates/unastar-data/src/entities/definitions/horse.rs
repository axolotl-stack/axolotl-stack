//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:horse`
pub struct Horse;
impl Horse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:horse`
#[derive(Bundle, Clone)]
pub struct HorseBundle {
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:horse` entity with default Bedrock components
pub fn spawn_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(HorseBundle {
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 1.6f32,
            },
            health: Health { value: 0, max: None },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
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
pub enum HorseComponentGroup {
    BaseBlack,
    BaseBrown,
    BaseChestnut,
    BaseCreamy,
    BaseDarkbrown,
    BaseGray,
    BaseWhite,
    HorseAdult,
    HorseBaby,
    HorseSaddled,
    HorseTamed,
    HorseWild,
    MarkingsBlackDots,
    MarkingsNone,
    MarkingsWhiteDetails,
    MarkingsWhiteDots,
    MarkingsWhiteFields,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HorseEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    HorseSaddled,
    HorseUnsaddled,
    MakeBlack,
    MakeBrown,
    MakeChestnut,
    MakeCreamy,
    MakeDarkbrown,
    MakeGray,
    MakeWhite,
    OnTame,
    SpawnAdult,
    SpawnTameAdult,
}
