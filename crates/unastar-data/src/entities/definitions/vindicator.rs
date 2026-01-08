//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:vindicator`
pub struct Vindicator;
impl Vindicator {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vindicator";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vindicator`
#[derive(Bundle, Clone)]
pub struct VindicatorBundle {
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
/// Spawn a new `minecraft:vindicator` entity with default Bedrock components
pub fn spawn_vindicator(commands: &mut Commands) -> Entity {
    commands
        .spawn(VindicatorBundle {
            attack: Attack {
                damage: 8i32,
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
pub enum VindicatorComponentGroup {
    Celebrate,
    DefaultTargeting,
    IllagerSquadCaptain,
    PatrolCaptain,
    PatrolFollower,
    RaidConfiguration,
    RaidDespawn,
    RaidPersistence,
    VindicatorAggro,
    VindicatorJohnny,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VindicatorEvent {
    BecomeAggro,
    EntitySpawned,
    PromoteToIllagerCaptain,
    PromoteToPatrolCaptain,
    RaidExpired,
    SpawnAsIllagerCaptain,
    SpawnAsPatrolFollower,
    SpawnForRaid,
    StartCelebrating,
    StartJohnny,
    StopAggro,
    StopCelebrating,
    StopJohnny,
}
