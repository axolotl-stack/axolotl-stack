//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:zombie`
pub struct Zombie;
impl Zombie {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:zombie";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:zombie`
#[derive(Bundle, Clone)]
pub struct ZombieBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stomp_turtle_egg: BehaviorStompTurtleEgg,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub rotation_locked_to_vehicle: RotationLockedToVehicle,
}
/// Spawn a new `minecraft:zombie` entity with default Bedrock components
pub fn spawn_zombie(commands: &mut Commands) -> Entity {
    commands
        .spawn(ZombieBundle {
            behavior_equip_item: BehaviorEquipItem {
                priority: Some(2i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(7i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stomp_turtle_egg: BehaviorStompTurtleEgg {
                goal_radius: Some(1.14f32),
                interval: Some(20i32),
                priority: Some(5i32),
                search_count: None,
                search_height: Some(2i32),
                search_range: Some(10i32),
                speed_multiplier: Some(1f32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
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
            rotation_locked_to_vehicle: RotationLockedToVehicle,
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieComponentGroup {
    CanBreakDoors,
    CanHaveEquipment,
    ConvertToBabyDrowned,
    ConvertToDrowned,
    LookToStartDrownedTransformation,
    NotOnZombieHorse,
    OnZombieHorse,
    StartDrownedTransformation,
    ZombieAdult,
    ZombieBaby,
    ZombieDefault,
    ZombieJockey,
    ZombieRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZombieEvent {
    AsAdult,
    AsBaby,
    AsBabyJockey,
    ConvertToDrowned,
    EntitySpawned,
    OnStartRidingZombieHorse,
    OnStopRidingZombieHorse,
    SpawnAsRider,
    StartTransforming,
    StopTransforming,
}
