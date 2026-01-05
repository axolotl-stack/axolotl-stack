//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:bogged`
pub struct Bogged;
impl Bogged {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bogged";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bogged`
#[derive(Bundle, Clone)]
pub struct BoggedBundle {
    pub breathable: Breathable,
    pub burns_in_daylight: BurnsInDaylight,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:bogged` entity with default Bedrock components
pub fn spawn_bogged(commands: &mut Commands) -> Entity {
    commands
        .spawn(BoggedBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: false,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            burns_in_daylight: BurnsInDaylight,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                width: 0.6f32,
                height: 1.9f32,
            },
            health: Health {
                value: 16i32,
                max: Some(16i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum BoggedComponentGroup {
    BoggedSheared,
    MeleeAttack,
    RangedAttack,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoggedEvent {
    BeSheared,
    EntitySpawned,
    MeleeMode,
    RangedMode,
}
