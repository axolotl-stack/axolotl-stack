//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:ravager`
pub struct Ravager;
impl Ravager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:ravager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:ravager`
#[derive(Bundle, Clone)]
pub struct RavagerBundle {
    pub attack: Attack,
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
/// Spawn a new `minecraft:ravager` entity with default Bedrock components
pub fn spawn_ravager(commands: &mut Commands) -> Entity {
    commands
        .spawn(RavagerBundle {
            attack: Attack {
                damage: 12i32,
                effect_name: None,
                effect_duration: None,
            },
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 1.95f32,
                height: 2.2f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 100i32,
                max: Some(100i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0f32 },
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
pub enum RavagerComponentGroup {
    Celebrate,
    EvokerRiderForRaid,
    Hostile,
    PillagerCaptainRider,
    PillagerRider,
    PillagerRiderForRaid,
    RaidConfiguration,
    RaidPersistence,
    VindicatorCaptainRider,
    VindicatorRider,
    Roaring,
    Stunned,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RavagerEvent {
    BecomeStunned,
    EndRoar,
    EntitySpawned,
    RaidExpired,
    SpawnForRaid,
    SpawnForRaidWithEvokerRider,
    SpawnForRaidWithPillagerRider,
    SpawnWithPillagerCaptainRider,
    SpawnWithPillagerRider,
    SpawnWithVindicatorCaptainRider,
    SpawnWithVindicatorRider,
    StartCelebrating,
    StartRoar,
    StopCelebrating,
}
