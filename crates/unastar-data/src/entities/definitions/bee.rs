//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:bee`
pub struct Bee;
impl Bee {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:bee";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:bee`
#[derive(Bundle, Clone)]
pub struct BeeBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction,
    pub collision_box: CollisionBox,
    pub flying_speed: FlyingSpeed,
    pub game_event_movement_tracking: GameEventMovementTracking,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_hover: MovementHover,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:bee` entity with default Bedrock components
pub fn spawn_bee(commands: &mut Commands) -> Entity {
    commands
        .spawn(BeeBundle {
            balloonable: Balloonable {
                mass: Some(0.5f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(19i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_move_towards_home_restriction: BehaviorMoveTowardsHomeRestriction {
                priority: Some(9i32),
                speed_multiplier: Some(1f32),
            },
            collision_box: CollisionBox {
                height: Some(0.5f32),
                width: Some(0.55f32),
            },
            flying_speed: FlyingSpeed { value: 0.15f32 },
            game_event_movement_tracking: GameEventMovementTracking {
                emit_flap: Some(true),
                emit_move: Some(true),
                emit_swim: Some(true),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_hover: MovementHover {
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
pub enum BeeComponentGroup {
    AbortShelterDetection,
    AddPoisonEffect,
    AddWitherEffect,
    AngryBee,
    BeeAdult,
    BeeBaby,
    CountdownToPerish,
    DefaultSound,
    EasyAttack,
    EscapeFire,
    FindHive,
    HardAttack,
    HasNectar,
    HiveFull,
    LookForFood,
    NormalAttack,
    Perish,
    ReturnToHome,
    ShelterDetection,
    TakeNearestTarget,
    TrackAttacker,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeeEvent {
    AbortSheltering,
    Attacked,
    CalmedDown,
    CollectedNectar,
    CountdownToPerishEvent,
    FedOpenEyeblossom,
    FedWitherRose,
    FindFlowerTimeout,
    FindHiveEvent,
    FindHiveTimeout,
    HiveDestroyed,
    AgeableGrowUp,
    EntityBorn,
    EntitySpawned,
    ExitedDisturbedHive,
    ExitedHive,
    ExitedHiveOnFire,
    HiveFull,
    SpawnAdult,
    OnPoisonEffectAdded,
    OnWitherEffectAdded,
    PerishEvent,
    SeekShelter,
    StopPanickingAfterFire,
}
