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
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub entity_sensor: EntitySensor,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:armadillo` entity with default Bedrock components
pub fn spawn_armadillo(commands: &mut Commands) -> Entity {
    commands
        .spawn(ArmadilloBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.65f32),
                width: Some(0.7f32),
            },
            entity_sensor: EntitySensor,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
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
