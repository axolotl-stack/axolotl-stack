//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:turtle`
pub struct Turtle;
impl Turtle {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:turtle";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:turtle`
#[derive(Bundle, Clone)]
pub struct TurtleBundle {
    pub behavior_move_to_water: BehaviorMoveToWater,
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_amphibious: MovementAmphibious,
    pub physics: Physics,
    pub pushable: Pushable,
    pub water_movement: WaterMovement,
}
/// Spawn a new `minecraft:turtle` entity with default Bedrock components
pub fn spawn_turtle(commands: &mut Commands) -> Entity {
    commands
        .spawn(TurtleBundle {
            behavior_move_to_water: BehaviorMoveToWater {
                goal_radius: Some(1.5f32),
                priority: Some(4i32),
                search_count: Some(10i32),
                search_height: Some(5i32),
                search_range: Some(16i32),
                speed_multiplier: Some(1f32),
            },
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(0i32),
                priority: Some(7i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(30i32),
                y_dist: Some(15i32),
            },
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_amphibious: MovementAmphibious {
                max_turn: Some(5f32),
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
            water_movement: WaterMovement {
                drag_factor: Some(0.9f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleComponentGroup {
    Adult,
    Baby,
    Pregnant,
    WantsToLayEgg,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurtleEvent {
    AgeableGrowUp,
    BecomePregnant,
    EntityBorn,
    EntitySpawned,
    GoLayEgg,
    LaidEgg,
}
