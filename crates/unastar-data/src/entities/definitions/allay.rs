//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:allay`
pub struct Allay;
impl Allay {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:allay";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:allay`
#[derive(Bundle, Clone)]
pub struct AllayBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_follow_owner: BehaviorFollowOwner,
    pub collision_box: CollisionBox,
    pub flying_speed: FlyingSpeed,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub leashable_to: LeashableTo,
    pub movement_hover: MovementHover,
    pub physics: Physics,
    pub pushable: Pushable,
    pub vibration_listener: VibrationListener,
}
/// Spawn a new `minecraft:allay` entity with default Bedrock components
pub fn spawn_allay(commands: &mut Commands) -> Entity {
    commands
        .spawn(AllayBundle {
            balloonable: Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(7i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_follow_owner: BehaviorFollowOwner {
                can_teleport: Some(false),
                ignore_vibration: Some(false),
                max_distance: Some(60f32),
                post_teleport_distance: Some(0.0),
                priority: Some(6i32),
                speed_multiplier: Some(8f32),
                start_distance: Some(16f32),
                stop_distance: Some(4f32),
            },
            collision_box: CollisionBox {
                height: Some(0.6f32),
                width: Some(0.35f32),
            },
            flying_speed: FlyingSpeed { value: 0.1f32 },
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(1i32),
                private: Some(false),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            movement_hover: MovementHover {
                max_turn: Some(30f32),
            },
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(false),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            vibration_listener: VibrationListener,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllayComponentGroup {
    PickupItem,
    PickupItemDelay,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllayEvent {
    EntitySpawned,
    PickupItemDelay,
    PickupItemDelayComplete,
}
