//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:sniffer`
pub struct Sniffer;
impl Sniffer {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:sniffer";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:sniffer`
#[derive(Bundle, Clone)]
pub struct SnifferBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:sniffer` entity with default Bedrock components
pub fn spawn_sniffer(commands: &mut Commands) -> Entity {
    commands
        .spawn(SnifferBundle {
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
                width: 1.9f32,
                height: 1.75f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 14i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.09f32 },
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
pub enum SnifferComponentGroup {
    FeelingHappy,
    Pushable,
    SnifferAdult,
    SnifferBaby,
    SnifferPregnant,
    SnifferSearchAndDig,
    StandUp,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnifferEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    OnDiggingStart,
    OnEggSpawned,
    OnFailDuringDigging,
    OnFailDuringSearching,
    OnFeelingHappyEnd,
    OnItemFound,
    OnPregnant,
    OnRisingEnd,
    OnScentingSuccess,
    OnSearchAndDiggingSuccess,
}
