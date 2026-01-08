//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:strider`
pub struct Strider;
impl Strider {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:strider";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:strider`
#[derive(Bundle, Clone)]
pub struct StriderBundle {
    pub collision_box: CollisionBox,
    pub fire_immune: FireImmune,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:strider` entity with default Bedrock components
pub fn spawn_strider(commands: &mut Commands) -> Entity {
    commands
        .spawn(StriderBundle {
            collision_box: CollisionBox {
                width: 0.9f32,
                height: 1.7f32,
            },
            fire_immune: FireImmune,
            health: Health {
                value: 20i32,
                max: Some(20i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.16f32 },
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
pub enum StriderComponentGroup {
    DetectSuffocating,
    StartSuffocating,
    StriderAdult,
    StriderBaby,
    StriderParentJockey,
    StriderPathingBehaviors,
    StriderPiglinJockey,
    StriderSaddled,
    StriderUnsaddled,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StriderEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    OnSaddled,
    OnUnsaddled,
    SpawnBabyStriderJockey,
    OnNotRidingParent,
    SpawnAdult,
    SpawnAdultParentJockey,
    SpawnAdultPiglinJockey,
    SpawnBaby,
    StartSuffocating,
    StopSuffocating,
}
