//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:creaking`
pub struct Creaking;
impl Creaking {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:creaking";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:creaking`
#[derive(Bundle, Clone)]
pub struct CreakingBundle {
    pub attack: Attack,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:creaking` entity with default Bedrock components
pub fn spawn_creaking(commands: &mut Commands) -> Entity {
    commands
        .spawn(CreakingBundle {
            attack: Attack {
                damage: 3i32,
                effect_name: None,
                effect_duration: None,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.9f32,
                height: 2.7f32,
            },
            follow_range: FollowRange { range: 32i32 },
            health: Health {
                value: 1i32,
                max: Some(1i32),
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
pub enum CreakingComponentGroup {
    Crumbling,
    Hostile,
    HostileUnobserved,
    Immobile,
    Mobile,
    Neutral,
    SpawnedByCreakingHeart,
    SpawnedByPlayer,
    Twitching,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreakingEvent {
    BecomeHostile,
    BecomeNeutral,
    Crumble,
    CrumbleAndNotifyCreakingHeart,
    DamagedByEntity,
    DamagedByPlayer,
    EntitySpawned,
    EntitySpawnedByCreakingHeart,
    IncrementSwayingTicks,
    OnTargetStartLooking,
    OnTargetStopLooking,
    ResetSwayingTicks,
    StartTwitching,
}
