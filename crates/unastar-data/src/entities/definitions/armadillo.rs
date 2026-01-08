//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:armadillo`
pub struct Armadillo;
impl Armadillo {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:armadillo";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:armadillo`
#[derive(Bundle, Clone)]
pub struct ArmadilloBundle {
    pub breathable: Breathable,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub health: Health,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub leashable: Leashable,
    pub nameable: Nameable,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:armadillo` entity with default Bedrock components
pub fn spawn_armadillo(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArmadilloBundle {
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
                width: 0.7f32,
                height: 0.65f32,
            },
            health: Health {
                value: 12i32,
                max: None,
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
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
pub enum ArmadilloComponentGroup {
    Adult,
    AdultUnrolled,
    Baby,
    BabyUnrolled,
    RolledUp,
    RolledUpWithThreats,
    RolledUpWithoutThreats,
    Unrolled,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmadilloEvent {
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    NoThreatDetected,
    RollUp,
    SpawnAdult,
    SpawnBaby,
    StartPeeking,
    StartUnrolling,
    StopPeeking,
    ThreatDetected,
    Unroll,
}
