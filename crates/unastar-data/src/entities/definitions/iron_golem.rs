//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:iron_golem`
pub struct IronGolem;
impl IronGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:iron_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:iron_golem`
#[derive(Bundle, Clone)]
pub struct IronGolemBundle {
    pub attack: Attack,
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
/// Spawn a new `minecraft:iron_golem` entity with default Bedrock components
pub fn spawn_iron_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(IronGolemBundle {
            attack: Attack {
                damage: 0,
                effect_name: None,
                effect_duration: None,
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 1.4f32,
                height: 2.9f32,
            },
            follow_range: FollowRange { range: 64i32 },
            health: Health {
                value: 100i32,
                max: Some(100i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.25f32 },
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
pub enum IronGolemComponentGroup {
    PlayerCreated,
    VillageCreated,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IronGolemEvent {
    FromPlayer,
    FromVillage,
}
