//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:parrot`
pub struct Parrot;
impl Parrot {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:parrot";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:parrot`
#[derive(Bundle, Clone)]
pub struct ParrotBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub collision_box: CollisionBox,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_fly: MovementFly,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:parrot` entity with default Bedrock components
pub fn spawn_parrot(commands: &mut Commands) -> Entity {
    commands
        .spawn(ParrotBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(1i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            collision_box: CollisionBox {
                height: Some(1f32),
                width: Some(0.5f32),
            },
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_fly: MovementFly {
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
pub enum ParrotComponentGroup {
    ParrotAdult,
    ParrotBlue,
    ParrotCyan,
    ParrotGreen,
    ParrotNotRidingPlayer,
    ParrotRed,
    ParrotRidingPlayer,
    ParrotSilver,
    ParrotTame,
    ParrotWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParrotEvent {
    EntitySpawned,
    OnNotRidingPlayer,
    OnRidingPlayer,
    OnTame,
}
