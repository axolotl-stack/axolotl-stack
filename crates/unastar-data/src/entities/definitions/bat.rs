//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:bat`
pub struct Bat;
impl Bat {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bat";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bat`
#[derive(Bundle, Clone)]
pub struct BatBundle {
    pub behavior_float: BehaviorFloat,
    pub collision_box: CollisionBox,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:bat` entity with default Bedrock components
pub fn spawn_bat(commands: &mut Commands) -> Entity {
    commands
        .spawn(BatBundle {
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            collision_box: CollisionBox {
                height: Some(0.9f32),
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
