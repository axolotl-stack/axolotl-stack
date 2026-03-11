//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:vex`
pub struct Vex;
impl Vex {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:vex";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:vex`
#[derive(Bundle, Clone)]
pub struct VexBundle {
    pub behavior_charge_attack: BehaviorChargeAttack,
    pub behavior_float: BehaviorFloat,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub experience_reward: ExperienceReward,
    pub fire_immune: FireImmune,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:vex` entity with default Bedrock components
pub fn spawn_vex(commands: &mut Commands) -> Entity {
    commands
        .spawn(VexBundle {
            behavior_charge_attack: BehaviorChargeAttack {
                max_distance: Some(3f32),
                min_distance: Some(2f32),
                priority: Some(4i32),
                speed_multiplier: Some(1f32),
                success_rate: Some(0.1428f32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.8f32),
                width: Some(0.4f32),
            },
            experience_reward: ExperienceReward {
                on_bred: None,
                on_death: Some(
                    "query.last_hit_by_player ? 5 + (query.equipment_count * Math.Random(1,3)) : 0"
                        .to_string(),
                ),
            },
            fire_immune: FireImmune,
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(false),
                emit_move: Some(false),
                emit_swim: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(false),
                has_gravity: Some(false),
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
pub enum VexComponentGroup {
    PeriodicDamage,
    StartDamageTimer,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VexEvent {
    AddDamageTimer,
    AddPeriodicDamage,
}
