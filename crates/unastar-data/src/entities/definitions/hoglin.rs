//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:hoglin`
pub struct Hoglin;
impl Hoglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:hoglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:hoglin`
#[derive(Bundle, Clone)]
pub struct HoglinBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:hoglin` entity with default Bedrock components
pub fn spawn_hoglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(HoglinBundle {
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
                height: 1.8f32,
            },
            health: Health {
                value: 40i32,
                max: Some(40i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
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
pub enum HoglinComponentGroup {
    AngryHoglin,
    AttackCooldown,
    BecomeZombie,
    HuntableAdult,
    HoglinAdult,
    HoglinBaby,
    StartZombification,
    UnhuntableAdult,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoglinEvent {
    AttackCooldownCompleteEvent,
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    EscapedEvent,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    SpawnAdultUnhuntable,
    SpawnBaby,
    StartZombificationEvent,
    StopZombificationEvent,
}
