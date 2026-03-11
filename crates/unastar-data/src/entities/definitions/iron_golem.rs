//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:iron_golem`
pub struct IronGolem;
impl IronGolem {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:iron_golem";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:iron_golem`
#[derive(Bundle, Clone)]
pub struct IronGolemBundle {
    pub balloonable: Balloonable,
    pub behavior_move_through_village: BehaviorMoveThroughVillage,
    pub behavior_move_towards_dwelling_restriction: BehaviorMoveTowardsDwellingRestriction,
    pub behavior_move_towards_target: BehaviorMoveTowardsTarget,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_climb: CanClimb,
    pub collision_box: CollisionBox,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub jump_static: JumpStatic,
    pub leashable_to: LeashableTo,
    pub loot: Loot,
    pub movement_basic: MovementBasic,
    pub persistent: Persistent,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:iron_golem` entity with default Bedrock components
pub fn spawn_iron_golem(commands: &mut Commands) -> Entity {
    commands
        .spawn(IronGolemBundle {
            balloonable: Balloonable {
                mass: Some(2f32),
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_move_through_village: BehaviorMoveThroughVillage {
                only_at_night: Some(true),
                priority: Some(3i32),
                speed_multiplier: Some(0.6f32),
            },
            behavior_move_towards_dwelling_restriction: BehaviorMoveTowardsDwellingRestriction {
                priority: Some(4i32),
                speed_multiplier: Some(1f32),
            },
            behavior_move_towards_target: BehaviorMoveTowardsTarget {
                priority: Some(2i32),
                speed_multiplier: Some(0.9f32),
                within_radius: Some(32f32),
            },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.6f32),
                xz_dist: Some(16i32),
                y_dist: Some(7i32),
            },
            can_climb: CanClimb,
            collision_box: CollisionBox {
                height: Some(2.9f32),
                width: Some(1.4f32),
            },
            is_hidden_when_invisible: IsHiddenWhenInvisible,
            jump_static: JumpStatic {
                jump_power: Some(0.42f32),
            },
            leashable_to: LeashableTo {
                can_retrieve_from: Some(false),
            },
            loot: Loot {
                table: "loot_tables/entities/iron_golem.json".to_string(),
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
pub enum IronGolemComponentGroup {
    PlayerCreated,
    VillageCreated,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IronGolemEvent {
    FromPlayer,
    FromVillage,
}
