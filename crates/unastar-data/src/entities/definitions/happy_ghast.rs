//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:happy_ghast`
pub struct HappyGhast;
impl HappyGhast {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:happy_ghast";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:happy_ghast`
#[derive(Bundle, Clone)]
pub struct HappyGhastBundle {
    pub can_fly: CanFly,
    pub collision_box: CollisionBox,
    pub follow_range: FollowRange,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub leashable: Leashable,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:happy_ghast` entity with default Bedrock components
pub fn spawn_happy_ghast(commands: &mut Commands) -> Entity {
    commands
        .spawn(HappyGhastBundle {
            can_fly: CanFly,
            collision_box: CollisionBox {
                width: 4f32,
                height: 4f32,
            },
            follow_range: FollowRange { range: 16i32 },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
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
pub enum HappyGhastComponentGroup {
    Adult,
    AdultHarnessed,
    AdultImmobile,
    AdultMobile,
    AdultUnharnessed,
    AdultWithPassengers,
    AdultWithoutPassengers,
    Baby,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HappyGhastEvent {
    AgeableGrowUp,
    BecomeImmobile,
    BecomeMobile,
    EntitySpawned,
    OnHarnessed,
    OnPassengerDismount,
    OnPassengerMount,
    OnStopTempting,
    OnUnharnessed,
    OnUnleashed,
    SpawnAdult,
    SpawnBaby,
}
