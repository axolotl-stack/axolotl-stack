//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:piglin`
pub struct Piglin;
impl Piglin {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:piglin";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:piglin`
#[derive(Bundle, Clone)]
pub struct PiglinBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:piglin` entity with default Bedrock components
pub fn spawn_piglin(commands: &mut Commands) -> Entity {
    commands
        .spawn(PiglinBundle {
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
                value: 16i32,
                max: Some(16i32),
            },
            inventory: Inventory {
                size: 8i32,
                container_type: None,
                can_be_siphoned_from: false,
                private: false,
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
pub enum PiglinComponentGroup {
    AlertForAttackTargets,
    Angry,
    AttackCooldown,
    BecomeZombie,
    Hunter,
    InteractablePiglin,
    MeleeUnit,
    NotHunter,
    PiglinAdult,
    PiglinBaby,
    PiglinJockey,
    RangedUnit,
    StartZombification,
    TakeTargetAsResponseToBlockBreak,
    ZombificationSensor,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiglinEvent {
    AdmireItemStartedEvent,
    AdmireItemStoppedEvent,
    AttackCooldownCompleteEvent,
    BecomeAngryEvent,
    BecomeCalmEvent,
    BecomeZombieEvent,
    ImportantBlockDestroyedEvent,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    SpawnAdultMelee,
    SpawnAdultMeleeNoHunting,
    SpawnAdultNoHunting,
    SpawnAdultRanged,
    SpawnAdultRangedNoHunting,
    SpawnBaby,
    StartZombificationEvent,
    StopZombificationEvent,
}
