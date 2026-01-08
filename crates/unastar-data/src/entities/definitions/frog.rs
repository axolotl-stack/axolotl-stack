//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:frog`
pub struct Frog;
impl Frog {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:frog";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:frog`
#[derive(Bundle, Clone)]
pub struct FrogBundle {
    pub breathable: Breathable,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub movement: Movement,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:frog` entity with default Bedrock components
pub fn spawn_frog(commands: &mut Commands) -> Entity {
    commands
        .spawn(FrogBundle {
            breathable: Breathable {
                total_supply: 15i32,
                suffocate_time: 0i32,
                breathes_air: true,
                breathes_water: true,
                breathes_lava: false,
                breathes_solids: false,
                generates_bubbles: false,
            },
            collision_box: CollisionBox {
                width: 0.5f32,
                height: 0.55f32,
            },
            health: Health {
                value: 10i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            leashable: Leashable,
            movement: Movement { speed: 0.1f32 },
            nameable: Nameable,
            physics: Physics {
                has_gravity: false,
                has_collision: false,
            },
            pushable: Pushable {
                is_pushable: false,
                is_pushable_by_piston: false,
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogComponentGroup {
    ColdFrog,
    Pregnant,
    TemperateFrog,
    WarmFrog,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrogEvent {
    BecomePregnant,
    LaidEgg,
    EntitySpawned,
    EntityTransformed,
    SpawnCold,
    SpawnTemperate,
    SpawnWarm,
}
