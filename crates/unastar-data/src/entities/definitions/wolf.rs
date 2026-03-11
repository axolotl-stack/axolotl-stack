//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:wolf`
pub struct Wolf;
impl Wolf {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:wolf";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:wolf`
#[derive(Bundle, Clone)]
pub struct WolfBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_leap_at_target: BehaviorLeapAtTarget,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub behavior_stay_while_sitting: BehaviorStayWhileSitting,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:wolf` entity with default Bedrock components
pub fn spawn_wolf(commands: &mut Commands) -> Entity {
    commands
        .spawn(WolfBundle {
            balloonable: Balloonable {
                mass: Some(0.8f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_float: BehaviorFloat {
                chance_per_tick_to_float: Some(0.8f32),
                priority: Some(0i32),
                sink_with_passengers: Some(false),
                time_under_water_to_dismount_passengers: Some(0f32),
            },
            behavior_leap_at_target: BehaviorLeapAtTarget {
                must_be_on_ground: Some(true),
                priority: Some(4i32),
                set_persistent: Some(false),
                target_dist: None,
                yd: Some(0.4f32),
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(1i32),
                speed_multiplier: Some(1.25f32),
                target_dist: Some(0f32),
                track_target: Some(true),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(8i32),
                speed_multiplier: Some(1f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            behavior_stay_while_sitting: BehaviorStayWhileSitting {
                priority: Some(3i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(0.8f32),
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
pub enum WolfComponentGroup {
    OnTameCollarColor,
    WolfAdult,
    WolfAngry,
    WolfArmorable,
    WolfAshen,
    WolfBaby,
    WolfBlack,
    WolfChestnut,
    WolfIncreasedMaxHealth,
    WolfLeashable,
    WolfPale,
    WolfRusty,
    WolfSnowy,
    WolfSpotted,
    WolfStriped,
    WolfTame,
    WolfWild,
    WolfWoods,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WolfEvent {
    AgeableGrowUp,
    AgeableSetBaby,
    BecomeAngry,
    BecomeArmorable,
    EntityBorn,
    EntitySpawned,
    IncreaseMaxHealth,
    OnCalm,
    OnTame,
    RandomizeSoundVariant,
    SpawnTameAdult,
    SpawnTameBaby,
    SpawnWildAdult,
    SpawnWildAshen,
    SpawnWildBaby,
    SpawnWildBabyOrAdult,
    SpawnWildBlack,
    SpawnWildChestnut,
    SpawnWildPale,
    SpawnWildRusty,
    SpawnWildSnowy,
    SpawnWildSpotted,
    SpawnWildStriped,
    SpawnWildWoods,
    UpgradeTo121100,
}
