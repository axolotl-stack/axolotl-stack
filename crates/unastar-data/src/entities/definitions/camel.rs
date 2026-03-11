//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:camel`
pub struct Camel;
impl Camel {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:camel";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:camel`
#[derive(Bundle, Clone)]
pub struct CamelBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_random_look_around_and_sit: BehaviorRandomLookAroundAndSit,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub variable_max_auto_step: VariableMaxAutoStep,
}
/// Spawn a new `minecraft:camel` entity with default Bedrock components
pub fn spawn_camel(commands: &mut Commands) -> Entity {
    commands
        .spawn(CamelBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(1f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(2f32),
            },
            behavior_random_look_around_and_sit: BehaviorRandomLookAroundAndSit {
                continue_if_leashed: Some(false),
                continue_sitting_on_reload: Some(false),
                max_angle_of_view_horizontal: Some(30f32),
                max_look_count: Some(2i32),
                max_look_time: Some(40i32),
                min_angle_of_view_horizontal: Some(-30f32),
                min_look_count: Some(1i32),
                min_look_time: Some(20i32),
                priority: None,
                probability: Some(0.02f32),
                random_look_around_cooldown: Some(0i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(2f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.375f32),
                width: Some(1.7f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            is_tamed: IsTamed,
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
            variable_max_auto_step: VariableMaxAutoStep {
                base_value: Some(1.5625f32),
                controlled_value: Some(1.5625f32),
                jump_prevented_value: Some(0.5625f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelComponentGroup {
    CamelAdult,
    CamelBaby,
    CamelSaddled,
    CamelSitting,
    CamelStanding,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CamelEvent {
    AgeableGrowUp,
    CamelSaddled,
    CamelUnsaddled,
    EntityBorn,
    EntitySpawned,
    SpawnAdult,
    StartSitting,
    StopSitting,
}
