//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:bee`
pub struct Bee;
impl Bee {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bee";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bee`
#[derive(Bundle, Clone)]
pub struct BeeBundle {
    pub breathable: Breathable,
    pub can_fly: CanFly,
    pub collision_box: CollisionBox,
    pub flying_speed: FlyingSpeed,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:bee` entity with default Bedrock components
pub fn spawn_bee(commands: &mut Commands) -> Entity {
    commands
        .spawn(BeeBundle {
            breathable: Breathable {
                total_supply: 0,
                suffocate_time: 0,
                breathes_air: false,
                breathes_water: false,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            can_fly: CanFly,
            collision_box: CollisionBox {
                width: 0.55f32,
                height: 0.5f32,
            },
            flying_speed: FlyingSpeed { speed: 0.15f32 },
            follow_range: FollowRange { range: 1024i32 },
            health: Health {
                value: 10i32,
                max: Some(10i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.3f32 },
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
pub enum BeeComponentGroup {
    AbortShelterDetection,
    AddPoisonEffect,
    AddWitherEffect,
    AngryBee,
    BeeAdult,
    BeeBaby,
    CountdownToPerish,
    DefaultSound,
    EasyAttack,
    EscapeFire,
    FindHive,
    HardAttack,
    HasNectar,
    HiveFull,
    LookForFood,
    NormalAttack,
    Perish,
    ReturnToHome,
    ShelterDetection,
    TakeNearestTarget,
    TrackAttacker,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeeEvent {
    AbortSheltering,
    Attacked,
    CalmedDown,
    CollectedNectar,
    CountdownToPerishEvent,
    FedOpenEyeblossom,
    FedWitherRose,
    FindFlowerTimeout,
    FindHiveEvent,
    FindHiveTimeout,
    HiveDestroyed,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    ExitedDisturbedHive,
    ExitedHive,
    ExitedHiveOnFire,
    HiveFull,
    SpawnAdult,
    OnPoisonEffectAdded,
    OnWitherEffectAdded,
    PerishEvent,
    SeekShelter,
    StopPanickingAfterFire,
}
