//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:husk`
pub struct Husk;
impl Husk {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:husk";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:husk`
#[derive(Bundle, Clone)]
pub struct HuskBundle {
    pub behavior_equip_item: BehaviorEquipItem,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stomp_turtle_egg: BehaviorStompTurtleEgg,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub rotation_locked_to_vehicle: RotationLockedToVehicle,
    pub variant: Variant,
}
/// Spawn a new `minecraft:husk` entity with default Bedrock components
pub fn spawn_husk(commands: &mut Commands) -> Entity {
    commands
        .spawn(HuskBundle {
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
            loot: Loot {
                table: "loot_tables/entities/zombie.json".to_string(),
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
            variant: Variant { value: 2i32 },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HuskComponentGroup {
    CanBreakDoors,
    ConvertToBabyZombie,
    ConvertToZombie,
    LookToStartZombieTransformation,
    NotOnCamelHusk,
    OnCamelHusk,
    StartZombieTransformation,
    ZombieHuskAdult,
    ZombieHuskBaby,
    ZombieHuskJockey,
    ZombieHuskRider,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HuskEvent {
    AsAdult,
    AsBaby,
    AsBabyJockey,
    ConvertToZombie,
    EntitySpawned,
    OnStartRidingCamelHusk,
    OnStopRidingCamelHusk,
    SpawnAsRider,
    StartTransforming,
    StopTransforming,
}
