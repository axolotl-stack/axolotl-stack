//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:rabbit`
pub struct Rabbit;
impl Rabbit {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:rabbit";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:rabbit`
#[derive(Bundle, Clone)]
pub struct RabbitBundle {
    pub balloonable: Balloonable,
    pub behavior_breed: BehaviorBreed,
    pub behavior_float: BehaviorFloat,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub block_climber: BlockClimber,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement_skip: MovementSkip,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
}
/// Spawn a new `minecraft:rabbit` entity with default Bedrock components
pub fn spawn_rabbit(commands: &mut Commands) -> Entity {
    commands
        .spawn(RabbitBundle {
            balloonable: Balloonable {
                mass: Some(0.4f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_breed: BehaviorBreed {
                priority: Some(2i32),
                speed_multiplier: Some(0.8f32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(2i32),
                y_dist: Some(1i32),
            },
            block_climber: BlockClimber,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.67f32),
                width: Some(0.67f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement_skip: MovementSkip {
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
            scale: Scale { value: 0.6f32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitComponentGroup {
    Adult,
    Baby,
    CoatBlack,
    CoatBrown,
    CoatDesert,
    CoatSalt,
    CoatSplotched,
    CoatWhite,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitEvent {
    GrowUp,
    InDesert,
    InSnow,
    EntityBorn,
    EntitySpawned,
}
