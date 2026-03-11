//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:panda`
pub struct Panda;
impl Panda {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:panda";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:panda`
#[derive(Bundle, Clone)]
pub struct PandaBundle {
    pub balloonable: Balloonable,
    pub behavior_breed: BehaviorBreed,
    pub behavior_float: BehaviorFloat,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_random_sitting: BehaviorRandomSitting,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub inventory: Inventory,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
    pub scale: Scale,
    pub variant: Variant,
    pub water_movement: WaterMovement,
}
/// Spawn a new `minecraft:panda` entity with default Bedrock components
pub fn spawn_panda(commands: &mut Commands) -> Entity {
    commands
        .spawn(PandaBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_breed: BehaviorBreed {
                priority: Some(3i32),
                speed_multiplier: Some(1f32),
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(5i32),
                speed_multiplier: Some(1.5f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_sitting: BehaviorRandomSitting {
                cooldown: Some(30f32),
                cooldown_time: Some(0f32),
                min_sit_time: Some(10f32),
                priority: Some(5i32),
                speed_multiplier: None,
                start_chance: Some(0.01f32),
                stop_chance: Some(0.3f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(14i32),
                speed_multiplier: Some(0.8f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(1.25f32),
                width: Some(1.3f32),
            },
            inventory: Inventory {
                additional_slots_per_strength: Some(0i32),
                can_be_siphoned_from: Some(false),
                container_type: Some("none".to_string()),
                inventory_size: Some(1i32),
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
            physics: Physics {
                has_collision: Some(true),
                has_gravity: Some(true),
                push_towards_closest_space: Some(false),
            },
            pushable: Pushable {
                is_pushable: Some(true),
                is_pushable_by_piston: Some(true),
            },
            scale: Scale { value: 1f32 },
            variant: Variant { value: 0i32 },
            water_movement: WaterMovement {
                drag_factor: Some(0.98f32),
            },
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaComponentGroup {
    BabyScared,
    PandaAdult,
    PandaAggressive,
    PandaAngry,
    PandaBaby,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaSneezing,
    PandaWeak,
    PandaWorried,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PandaEvent {
    AgeableGrowUp,
    BabyOnCalm,
    BecomeAngry,
    EntityBorn,
    EntitySpawned,
    OnCalm,
    OnScared,
    PandaAggressive,
    PandaBrown,
    PandaLazy,
    PandaPlayful,
    PandaWeak,
    PandaWorried,
}
