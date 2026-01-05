//! Generated definition for entity.
use bevy_ecs::prelude::*;
use super::super::components::*;
/// Entity definition for `minecraft:goat`
pub struct Goat;
impl Goat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:goat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:goat`
#[derive(Bundle, Clone)]
pub struct GoatBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:goat` entity with default Bedrock components
pub fn spawn_goat(commands: &mut Commands) -> Entity {
    commands
        .spawn(GoatBundle {
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
                width: 0.9f32,
                height: 1.3f32,
            },
            health: Health {
                value: 10i32,
                max: Some(10i32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.4f32 },
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
pub enum GoatComponentGroup {
    AttackCooldown,
    GoatAdult,
    GoatBaby,
    GoatDefault,
    GoatScreamer,
    InteractDefault,
    InteractScreamer,
    RamDefault,
    RamScreamer,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GoatEvent {
    AttackCooldownCompleteEvent,
    AgeableGrowUp,
    BornDefault,
    BornScreamer,
    EntityBorn,
    EntitySpawned,
    StartEvent,
}
