//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:pillager`
pub struct Pillager;
impl Pillager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:pillager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:pillager`
#[derive(Bundle, Clone)]
pub struct PillagerBundle {
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
/// Spawn a new `minecraft:pillager` entity with default Bedrock components
pub fn spawn_pillager(commands: &mut Commands) -> Entity {
    commands
        .spawn(PillagerBundle {
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
                width: 0.6f32,
                height: 1.9f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 24i32,
                max: Some(24i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement: Movement { speed: 0.35f32 },
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
pub enum PillagerComponentGroup {
    Celebrate,
    IllagerSquadCaptain,
    MeleeAttack,
    PatrolCaptain,
    PatrolFollower,
    RaidConfiguration,
    RaidPersistence,
    RangedAttack,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PillagerEvent {
    Calm,
    EntitySpawned,
    MeleeMode,
    PromoteToIllagerCaptain,
    PromoteToPatrolCaptain,
    RaidExpired,
    RangedMode,
    SpawnAsIllagerCaptain,
    SpawnAsPatrolFollower,
    SpawnForRaid,
    StartCelebrating,
    StopCelebrating,
}
