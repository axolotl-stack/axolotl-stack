//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:villager`
pub struct Villager;
impl Villager {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:villager";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:villager`
#[derive(Bundle, Clone)]
pub struct VillagerBundle {
    pub annotation_open_door: AnnotationOpenDoor,
    pub behavior_float: BehaviorFloat,
    pub behavior_move_indoors: BehaviorMoveIndoors,
    pub behavior_open_door: BehaviorOpenDoor,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_restrict_open_door: BehaviorRestrictOpenDoor,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:villager` entity with default Bedrock components
pub fn spawn_villager(commands: &mut Commands) -> Entity {
    commands
        .spawn(VillagerBundle {
            annotation_open_door: AnnotationOpenDoor,
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_move_indoors: BehaviorMoveIndoors {
                priority: Some(4i32),
                speed_multiplier: Some(0.8f32),
                timeout_cooldown: Some(8f32),
            },
            behavior_open_door: BehaviorOpenDoor {
                close_door_after: Some(true),
                priority: Some(6i32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(11i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_restrict_open_door: BehaviorRestrictOpenDoor {
                priority: Some(5i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.9f32),
                width: Some(0.6f32),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(8i32),
                private: Some(true),
                restrict_to_owner: Some(false),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            movement_basic: MovementBasic {
                max_turn: Some(30f32),
            },
            persistent: Persistent,
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
pub enum VillagerComponentGroup {
    Adult,
    Armorer,
    Baby,
    BecomeVillagerV2,
    BecomeWitch,
    BecomeZombie,
    BehaviorNonPeasant,
    BehaviorPeasant,
    Butcher,
    Cartographer,
    Cleric,
    Farmer,
    Fisherman,
    Fletcher,
    Leatherworker,
    Librarian,
    Celebrate,
    Shepherd,
    Toolsmith,
    Weaponsmith,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VillagerEvent {
    BecomeWitch,
    BecomeZombie,
    AgeableGrowUp,
    BecomeCleric,
    EntityBorn,
    EntitySpawned,
    EntityTransformed,
    SpawnArmorer,
    SpawnButcher,
    SpawnCleric,
    SpawnFarmer,
    SpawnLibrarian,
    StartCelebrating,
    StopCelebrating,
}
