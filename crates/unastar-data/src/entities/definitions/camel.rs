//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:camel`
pub struct Camel;
impl Camel {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:camel";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:camel`
#[derive(Bundle, Clone)]
pub struct CamelBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:camel` entity with default Bedrock components
pub fn spawn_camel(commands: &mut Commands) -> Entity {
    commands
        .spawn(CamelBundle {
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
                width: 1.7f32,
                height: 2.375f32,
            },
            health: Health {
                value: 32i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
            leashable: Leashable,
            movement: Movement { speed: 0.09f32 },
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
pub enum CamelComponentGroup {
    CamelAdult,
    CamelBaby,
    CamelSaddled,
    CamelSitting,
    CamelStanding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelEvent {
    AgeableGrowUp,
    CamelSaddled,
    CamelUnsaddled,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    StartSitting,
    StopSitting,
}
