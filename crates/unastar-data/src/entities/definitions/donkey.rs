//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:donkey`
pub struct Donkey;
impl Donkey {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:donkey";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:donkey`
#[derive(Bundle, Clone)]
pub struct DonkeyBundle {
    pub balloonable: Balloonable,
    pub behavior_float: BehaviorFloat,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:donkey` entity with default Bedrock components
pub fn spawn_donkey(commands: &mut Commands) -> Entity {
    commands
        .spawn(DonkeyBundle {
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
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.7f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            collision_box: CollisionBox {
                height: Some(1.6f32),
                width: Some(1.4f32),
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
pub enum DonkeyComponentGroup {
    DonkeyAdult,
    DonkeyBaby,
    DonkeyChested,
    DonkeySaddled,
    DonkeyTamed,
    DonkeyUnchested,
    DonkeyWild,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DonkeyEvent {
    AgeableGrowUp,
    DonkeySaddled,
    DonkeyUnsaddled,
    EntityBorn,
    EntitySpawned,
    OnChest,
    OnTame,
    SpawnAdult,
    SpawnTameAdult,
}
