//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:tadpole`
pub struct Tadpole;
impl Tadpole {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:tadpole";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:tadpole`
#[derive(Bundle, Clone)]
pub struct TadpoleBundle {
    pub behavior_random_swim: BehaviorRandomSwim,
    pub collision_box: CollisionBox,
    pub is_baby: IsBaby,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub movement_sway: MovementSway,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:tadpole` entity with default Bedrock components
pub fn spawn_tadpole(commands: &mut Commands) -> Entity {
    commands
        .spawn(TadpoleBundle {
            behavior_random_swim: BehaviorRandomSwim {
                avoid_surface: Some(true),
                interval: Some(100i32),
                priority: Some(2i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(0.6f32),
                width: Some(0.8f32),
            },
            is_baby: IsBaby,
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            movement_sway: MovementSway {
                max_turn: Some(30f32),
                sway_amplitude: Some(0f32),
                sway_frequency: Some(0.5f32),
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
pub enum TadpoleComponentGroup {
    GrowUp,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TadpoleEvent {
    AgeableGrowUp,
}
