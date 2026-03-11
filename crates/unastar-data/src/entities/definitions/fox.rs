//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:fox`
pub struct Fox;
impl Fox {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:fox";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:fox`
#[derive(Bundle, Clone)]
pub struct FoxBundle {
    pub balloonable: Balloonable,
    pub behavior_eat_carried_item: BehaviorEatCarriedItem,
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_float: BehaviorFloat,
    pub behavior_random_look_around_and_sit: BehaviorRandomLookAroundAndSit,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub block_climber: BlockClimber,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:fox` entity with default Bedrock components
pub fn spawn_fox(commands: &mut Commands) -> Entity {
    commands
        .spawn(FoxBundle {
            balloonable: Balloonable {
                mass: Some(0.6f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_eat_carried_item: BehaviorEatCarriedItem {
                delay_before_eating: Some(28f32),
                priority: Some(12i32),
            },
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(2i32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_random_look_around_and_sit: BehaviorRandomLookAroundAndSit {
                continue_if_leashed: Some(false),
                continue_sitting_on_reload: Some(false),
                max_angle_of_view_horizontal: Some(30f32),
                max_look_count: Some(5i32),
                max_look_time: Some(100i32),
                min_angle_of_view_horizontal: Some(-30f32),
                min_look_count: Some(2i32),
                min_look_time: Some(80i32),
                priority: Some(12i32),
                probability: Some(0.001f32),
                random_look_around_cooldown: Some(0i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(13i32),
                speed_multiplier: Some(0.8f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            block_climber: BlockClimber,
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.7f32),
                width: Some(0.6f32),
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
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxComponentGroup {
    DefendingFox,
    DocileFox,
    FoxAdult,
    FoxAmbientDefendingTarget,
    FoxAmbientNight,
    FoxAmbientNormal,
    FoxAmbientSleep,
    FoxArctic,
    FoxBaby,
    FoxDay,
    FoxNight,
    FoxRed,
    FoxThunderstorm,
    FoxWithItem,
    TrustingFox,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxEvent {
    AgeableGrowUp,
    AmbientNight,
    AmbientNormal,
    AmbientSleep,
    EntityBorn,
    EntitySpawned,
    FoxConfigureDay,
    FoxConfigureDefending,
    FoxConfigureDocileDay,
    FoxConfigureDocileNight,
    FoxConfigureNight,
    FoxConfigureThunderstorm,
}
