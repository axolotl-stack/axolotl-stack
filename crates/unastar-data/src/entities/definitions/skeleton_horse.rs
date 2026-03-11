//! Generated definition for entity.
use super::super::components::*;
use bevy_ecs::prelude::*;
/// Entity definition for `minecraft:skeleton_horse`
pub struct SkeletonHorse;
impl SkeletonHorse {
    /// The entity identifier
    pub const IDENTIFIER: &'static str = "minecraft:skeleton_horse";
    /// Whether this entity can spawn naturally
    pub const IS_SPAWNABLE: bool = true;
    /// Whether this entity can be summoned via commands
    pub const IS_SUMMONABLE: bool = true;
}
/// Component bundle for spawning a `minecraft:skeleton_horse`
#[derive(Bundle, Clone)]
pub struct SkeletonHorseBundle {
    pub balloonable: Balloonable,
    pub behavior_mount_pathing: BehaviorMountPathing,
    pub behavior_player_ride_tamed: BehaviorPlayerRideTamed,
    pub behavior_random_stroll: BehaviorRandomStroll,
    pub can_power_jump: CanPowerJump,
    pub collision_box: CollisionBox,
    pub input_ground_controlled: InputGroundControlled,
    pub is_hidden_when_invisible: IsHiddenWhenInvisible,
    pub is_tamed: IsTamed,
    pub jump_static: JumpStatic,
    pub movement_basic: MovementBasic,
    pub physics: Physics,
    pub pushable: Pushable,
}
/// Spawn a new `minecraft:skeleton_horse` entity with default Bedrock components
pub fn spawn_skeleton_horse(commands: &mut Commands) -> Entity {
    commands
        .spawn(SkeletonHorseBundle {
            balloonable: Balloonable {
                mass: None,
                max_distance: None,
                on_balloon: None,
                on_unballoon: None,
                soft_distance: None,
            },
            behavior_mount_pathing: BehaviorMountPathing {
                priority: Some(2i32),
                speed_multiplier: Some(1.5f32),
                target_dist: Some(4f32),
                track_target: Some(true),
            },
            behavior_player_ride_tamed: BehaviorPlayerRideTamed { priority: None },
            behavior_random_stroll: BehaviorRandomStroll {
                interval: Some(120i32),
                priority: Some(6i32),
                speed_multiplier: Some(0.7f32),
                xz_dist: Some(10i32),
                y_dist: Some(7i32),
            },
            can_power_jump: CanPowerJump,
            collision_box: CollisionBox {
                height: Some(1.8f32),
                width: Some(0.6f32),
            },
            input_ground_controlled: InputGroundControlled,
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
        })
        .id()
}
/// Component groups for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkeletonHorseComponentGroup {
    LightningImmune,
    SkeletonHorseAdult,
    SkeletonHorseBaby,
    SkeletonHorseR5Upgrade,
    SkeletonTrap,
}
/// Events for this entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkeletonHorseEvent {
    EntitySpawned,
    SetTrap,
    SpringTrap,
}
